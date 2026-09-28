pub fn normalize(command: &str) -> String {
    command
        .chars()
        .filter(|c| *c != '\\')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn requires_root(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    c.split([';', '&', '|', '(', '`']).any(|segment| {
        segment
            .trim_start_matches([' ', '\'', '"', '$'])
            .starts_with("su ")
    }) || c.contains("/data/system")
        || (c.contains("/system/") && has_mutation(&c))
        || c.contains("/dev/block")
        || c.contains(" remount")
}

pub fn has_mutation(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    // Make command names inside substitutions visible to the conservative token
    // check without treating every benign `$(...)` query as a side effect.
    let lexical = c.replace("$(", " ").replace(['(', ')', '`'], " ");
    let tokens = shell_words::split(&lexical)
        .unwrap_or_else(|_| lexical.split_whitespace().map(str::to_owned).collect());
    let mutating = [
        "rm", "mv", "cp", "chmod", "chown", "mkdir", "rmdir", "touch", "ln", "truncate", "tee",
        "setprop", "kill", "pkill", "reboot", "shutdown", "halt", "poweroff", "mkfs", "dd",
    ];
    tokens.iter().any(|t| mutating.contains(&t.as_str()))
        || c.contains("sed -i")
        || c.contains("-delete")
        || c.contains("settings put")
        || c.contains("pm install")
        || c.contains("pm uninstall")
        || has_mutating_redirection(&c)
        || c.contains("mount -o")
}

fn has_mutating_redirection(command: &str) -> bool {
    let positions = if shell_reparses_arguments(command) {
        command.match_indices('>').map(|(index, _)| index).collect()
    } else {
        redirection_positions(command)
    };
    positions.into_iter().any(|index| {
        let mut target = command[index + 1..].trim_start();
        if let Some(rest) = target.strip_prefix('>') {
            target = rest.trim_start();
        }
        !is_discard_target(target) && !is_fd_duplication(target)
    })
}

fn shell_reparses_arguments(command: &str) -> bool {
    let value = command.to_ascii_lowercase();
    if ["eval ", "system(", "| sh", "| bash", "| su"]
        .iter()
        .any(|marker| value.contains(marker))
    {
        return true;
    }

    // Bind code-execution options to known interpreters. In particular, grep's
    // -E/-e/-c and sed's -e must not expose quoted patterns as shell redirections.
    // This remains conservative token detection, not a complete shell parser.
    let mut lexical = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, ';' | '&' | '|' | '(' | ')' | '`') {
            lexical.push(' ');
            lexical.push(character);
            lexical.push(' ');
        } else {
            lexical.push(character);
        }
    }
    // Inspect quoted code too: an interpreter may be nested inside a command
    // substitution in a double-quoted argument.
    let tokens: Vec<_> = lexical
        .split_whitespace()
        .map(|token| token.trim_matches(['\'', '"']))
        .collect();
    tokens.iter().enumerate().any(|(index, token)| {
        let name = token.rsplit('/').next().unwrap_or(token);
        let code_option = match name {
            "sh" | "bash" | "dash" | "ash" | "ksh" | "mksh" | "zsh" | "su" => 'c',
            "perl" | "ruby" | "node" | "nodejs" | "awk" | "gawk" | "lua" | "luajit" | "r"
            | "rscript" => 'e',
            name if name
                .strip_prefix("python")
                .is_some_and(|version| version.chars().all(|c| c.is_ascii_digit() || c == '.')) =>
            {
                'c'
            }
            _ => return false,
        };
        tokens[index + 1..]
            .iter()
            // Options may have values before -c/-e (e.g. bash --rcfile path -c).
            // Stop at a compound-command boundary instead of the first value.
            .take_while(|argument| !matches!(**argument, "--" | ";" | "&" | "|" | "(" | ")" | "`"))
            .any(|argument| {
                argument.strip_prefix('-').is_some_and(|options| {
                    !options.starts_with('-') && options.contains(code_option)
                })
            })
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
}

fn redirection_positions(command: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut quote = Quote::None;
    let mut substitutions: Vec<(Quote, usize)> = Vec::new();
    let mut backtick_return = None;
    let mut chars = command.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        if character == '\\' && quote != Quote::Single {
            chars.next();
            continue;
        }
        match quote {
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                }
            }
            Quote::Double => match character {
                '"' => quote = Quote::None,
                '$' if chars.peek().is_some_and(|(_, next)| *next == '(') => {
                    chars.next();
                    substitutions.push((Quote::Double, 1));
                    quote = Quote::None;
                }
                '`' => {
                    backtick_return = Some(Quote::Double);
                    quote = Quote::None;
                }
                _ => {}
            },
            Quote::None => match character {
                '\'' => quote = Quote::Single,
                '"' => quote = Quote::Double,
                '`' => {
                    if let Some(previous) = backtick_return.take() {
                        quote = previous;
                    } else {
                        backtick_return = Some(Quote::None);
                    }
                }
                '$' if chars.peek().is_some_and(|(_, next)| *next == '(') => {
                    chars.next();
                    substitutions.push((Quote::None, 1));
                }
                '(' => {
                    if let Some((_, depth)) = substitutions.last_mut() {
                        *depth += 1;
                    }
                }
                ')' => {
                    if let Some((_, depth)) = substitutions.last_mut() {
                        *depth -= 1;
                        if *depth == 0 {
                            if let Some((previous, _)) = substitutions.pop() {
                                quote = previous;
                            }
                        }
                    }
                }
                '>' => positions.push(index),
                _ => {}
            },
        }
    }
    positions
}

fn is_discard_target(target: &str) -> bool {
    let (target, quote) = match target.chars().next() {
        Some(quote @ ('\'' | '"')) => (&target[quote.len_utf8()..], Some(quote)),
        _ => (target, None),
    };
    let Some(rest) = target.strip_prefix("/dev/null") else {
        return false;
    };
    let rest = match quote {
        Some(quote) => match rest.strip_prefix(quote) {
            Some(rest) => rest,
            None => return false,
        },
        None => rest,
    };
    is_shell_boundary(rest)
}

fn is_fd_duplication(target: &str) -> bool {
    let Some(rest) = target.strip_prefix('&') else {
        return false;
    };
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && is_shell_boundary(&rest[digits..])
}

fn is_shell_boundary(rest: &str) -> bool {
    match rest.chars().next() {
        None => true,
        Some(c) => c.is_whitespace() || matches!(c, ';' | '|' | '&' | ')' | '`'),
    }
}
