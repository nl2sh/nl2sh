use nl2sh::{
    config::{Config, ExecuteUserMode, SecurityLevel},
    security::{assess, RiskLevel},
};

#[test]
fn required_security_matrix() {
    let cfg = Config::default();
    let cases = [
        ("ls -la", RiskLevel::ReadOnly, false),
        ("cat /proc/cpuinfo", RiskLevel::ReadOnly, false),
        ("cat file > other", RiskLevel::Mutating, false),
        ("find /data -type f", RiskLevel::ReadOnly, false),
        ("find /data -type f -delete", RiskLevel::Mutating, false),
        ("rm -rf /", RiskLevel::Critical, true),
        ("rm -rf /*", RiskLevel::Critical, true),
        ("rm -r -f /", RiskLevel::Critical, true),
        ("r\\m -rf /", RiskLevel::Critical, true),
        ("rm -rf '/system'", RiskLevel::Critical, true),
        ("rm -rf /data/*", RiskLevel::Critical, true),
        ("mkfs.ext4 /dev/block/x", RiskLevel::Critical, true),
        ("dd if=/dev/zero of=/dev/block/x", RiskLevel::Critical, true),
        ("chmod -R 777 /", RiskLevel::Critical, true),
        ("reboot", RiskLevel::Dangerous, true),
        ("su -c 'rm -rf /'", RiskLevel::Critical, true),
        ("sh -c \"reboot\"", RiskLevel::Dangerous, true),
        ("echo $(rm -rf /)", RiskLevel::Critical, true),
        ("echo `reboot`", RiskLevel::Dangerous, true),
    ];
    for (command, risk, double) in cases {
        let a = assess(command, &cfg);
        assert_eq!(a.risk_level, risk, "{command}");
        assert_eq!(a.requires_double_confirmation, double, "{command}");
    }
}

#[test]
fn custom_rules_extend_builtins() {
    let mut cfg = Config::default();
    cfg.security_rules.push(nl2sh::config::SecurityRuleConfig {
        id: "custom".into(),
        pattern: r"secret-action".into(),
        risk: "dangerous".into(),
        message: "custom risk".into(),
    });
    let a = assess("secret-action", &cfg);
    assert_eq!(a.risk_level, RiskLevel::Dangerous);
    assert_eq!(a.matched_rules[0].id, "custom");
}

#[test]
fn configured_user_mode_controls_actual_root_plan() {
    let mut cfg = Config {
        execute_user_mode: ExecuteUserMode::Normal,
        ..Config::default()
    };
    assert!(!assess("cat /data/system/packages.xml", &cfg).requires_root);
    cfg.execute_user_mode = ExecuteUserMode::Root;
    assert!(assess("id", &cfg).requires_root);
    cfg.execute_user_mode = ExecuteUserMode::Auto;
    assert!(!assess("cat /system/build.prop", &cfg).requires_root);
    assert!(assess("su -c id", &cfg).requires_confirmation);
}

#[test]
fn invalid_runtime_rule_fails_closed() {
    let mut cfg = Config::default();
    cfg.security_rules.push(nl2sh::config::SecurityRuleConfig {
        id: "broken".into(),
        pattern: "(".into(),
        risk: "read_only".into(),
        message: "broken".into(),
    });
    let assessment = assess("ls", &cfg);
    assert_eq!(assessment.risk_level, RiskLevel::Critical);
    assert!(assessment.requires_double_confirmation);
}

#[test]
fn read_only_android_package_version_queries_do_not_require_confirmation() {
    let cfg = Config::default();
    for command in [
        "dumpsys package com.example.app | grep versionName",
        "pm list packages --show-versioncode",
        "for pkg in $(pm list packages -3 | cut -d: -f2); do dumpsys package $pkg | grep versionName; done",
    ] {
        let assessment = assess(command, &cfg);
        assert_eq!(assessment.risk_level, RiskLevel::ReadOnly, "{command}");
        assert!(!assessment.requires_confirmation, "{command}");
    }
}

#[test]
fn quoted_output_and_mount_listing_stay_read_only_without_hiding_writes() {
    let cfg = Config::default();
    for command in [
        "echo \"OK tool -> /system/bin/tool\"",
        "for t in toybox curl; do if command -v $t >/dev/null 2>&1; then echo \"OK $t -> $(command -v $t)\"; else echo \"MISS $t\"; fi; done",
        "mount | grep -E '/system|/vendor' | head",
        "mount 2>/dev/null",
    ] {
        let assessment = assess(command, &cfg);
        assert_eq!(assessment.risk_level, RiskLevel::ReadOnly, "{command}");
        assert!(!assessment.requires_confirmation, "{command}");
    }
    for command in [
        "echo x > /data/local/tmp/output",
        "echo \"safe -> text\" > /data/local/tmp/output",
        "echo x->/data/local/tmp/output",
        "echo \"$(echo x > /data/local/tmp/output)\"",
        "echo \"`echo x > /data/local/tmp/output`\"",
        "sh -c 'echo x > /data/local/tmp/output'",
        "eval 'echo x > /data/local/tmp/output'",
        "echo 'echo x > /data/local/tmp/output' | sh",
        "mount -t tmpfs tmpfs /mnt",
        "sh -c 'mount -t tmpfs tmpfs /mnt'",
    ] {
        let assessment = assess(command, &cfg);
        assert!(assessment.risk_level >= RiskLevel::Mutating, "{command}");
        assert!(assessment.requires_confirmation, "{command}");
    }
}

const TOMBSTONE_QUERY: &str = r#"ls -la /data/tombstones/ 2>/dev/null | head -n 30; for f in /data/tombstones/tombstone_0*; do case "$f" in *.pb) continue;; esac; echo "=== $f"; grep -m3 -E "pid:|>>> |Cmdline|signal " "$f" | head -n 5; done"#;

#[test]
fn diagnostic_options_do_not_reparse_quoted_patterns() {
    let cfg = Config::default();
    for command in [
        TOMBSTONE_QUERY,
        r#"grep -E "pid:|>>> |Cmdline|signal " tombstone"#,
        "grep -e '>>>' tombstone",
        "grep -c '>>>' tombstone",
        "sed -e 's/>>>/signal/g' tombstone",
    ] {
        let assessment = assess(command, &cfg);
        assert_eq!(assessment.risk_level, RiskLevel::ReadOnly, "{command}");
        assert!(!assessment.requires_confirmation, "{command}");
        assert!(!assessment.requires_root, "{command}");
    }
    let strict = Config {
        security_level: SecurityLevel::Strict,
        ..cfg
    };
    let assessment = assess(TOMBSTONE_QUERY, &strict);
    assert_eq!(assessment.risk_level, RiskLevel::ReadOnly);
    assert!(assessment.requires_confirmation);
}

#[test]
fn diagnostic_patterns_do_not_hide_real_or_reparsed_writes() {
    let cfg = Config::default();
    for command in [
        format!("{TOMBSTONE_QUERY} > /data/local/tmp/output"),
        r#"grep -E "$(echo x > /data/local/tmp/output)" tombstone"#.into(),
        "grep -e '`echo x > /data/local/tmp/output`' tombstone > /data/local/tmp/result".into(),
        "sh -c 'echo x > /data/local/tmp/output'".into(),
        "/system/bin/sh -lc 'echo x > /data/local/tmp/output'".into(),
        "env sh -x -c 'echo x > /data/local/tmp/output'".into(),
        "bash --rcfile /data/local/tmp/bashrc -c 'echo x > /data/local/tmp/output'".into(),
        "toybox sh -ec 'echo x > /data/local/tmp/output'".into(),
        "true;sh -c 'echo x > /data/local/tmp/output'".into(),
        r#"echo "$(sh -c 'echo x > /data/local/tmp/output')""#.into(),
        r#"echo "`sh -c 'echo x > /data/local/tmp/output'`""#.into(),
        r#"python3 -c 'open("/data/local/tmp/output", "w").write("x > y")'"#.into(),
        r#"/usr/bin/python3.11 -c 'open("/data/local/tmp/output", "w").write("x > y")'"#.into(),
        r#"perl -e 'open(my $f, ">", "/data/local/tmp/output"); print $f "x"'"#.into(),
        r#"perl -I /data/local/tmp/lib -e 'open(my $f, ">", "/data/local/tmp/output")'"#.into(),
        r#"node -e 'require("fs").writeFileSync("/data/local/tmp/output", "x > y")'"#.into(),
        r#"lua -e 'io.open("/data/local/tmp/output", "w"):write("x > y")'"#.into(),
        "eval 'echo x > /data/local/tmp/output'".into(),
        "echo 'echo x > /data/local/tmp/output' | sh".into(),
    ] {
        let assessment = assess(&command, &cfg);
        assert!(assessment.risk_level >= RiskLevel::Mutating, "{command}");
        assert!(assessment.requires_confirmation, "{command}");
    }
}

#[test]
fn mutating_commands_inside_substitutions_still_require_confirmation() {
    let cfg = Config::default();
    for command in [
        "echo $(touch /data/local/tmp/x)",
        "echo `rm /data/local/tmp/x`",
    ] {
        let assessment = assess(command, &cfg);
        assert!(assessment.risk_level >= RiskLevel::Mutating, "{command}");
        assert!(assessment.requires_confirmation, "{command}");
    }
}

#[test]
fn diagnostic_redirections_do_not_turn_queries_into_mutations() {
    let cfg = Config::default();
    for command in [
        "du -sh /data/user/0/com.example 2>/dev/null",
        "ls -lh /system/app/Example/ 2>/dev/null",
        "dumpsys package com.example >/dev/null",
        "dumpsys package com.example 2>&1",
        "dumpsys package com.example 2>> '/dev/null'",
    ] {
        let assessment = assess(command, &cfg);
        assert_eq!(assessment.risk_level, RiskLevel::ReadOnly, "{command}");
        assert!(!assessment.requires_root, "{command}");
        assert!(!assessment.requires_confirmation, "{command}");
    }
}

#[test]
fn real_writes_and_mutating_commands_with_discarded_stderr_stay_protected() {
    let cfg = Config::default();
    for command in [
        "echo data > /data/local/tmp/output",
        "echo data >/dev/null.backup",
        "echo data >'/dev/null'backup",
        "touch /data/local/tmp/file 2>/dev/null",
        "rm /data/local/tmp/file 2>&1",
    ] {
        let assessment = assess(command, &cfg);
        assert!(assessment.risk_level >= RiskLevel::Mutating, "{command}");
        assert!(assessment.requires_confirmation, "{command}");
    }
}

#[test]
fn strict_still_confirms_queries_after_correct_readonly_classification() {
    let cfg = Config {
        security_level: SecurityLevel::Strict,
        ..Config::default()
    };
    let assessment = assess("du -sh /system/app 2>/dev/null", &cfg);
    assert_eq!(assessment.risk_level, RiskLevel::ReadOnly);
    assert!(!assessment.requires_root);
    assert!(assessment.requires_confirmation);
}
