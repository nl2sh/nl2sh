//! Conservative shell effects extracted from the brush syntax tree.

use super::{
    effects::Effects,
    expansion::{static_word, visit_pieces, visit_word},
    parser::visit_source,
};
use crate::security::{policy, RiskLevel};
use brush_parser::{
    ast::{self, CommandPrefixOrSuffixItem as Item, IoFileRedirectKind as RedirectKind},
    word, ParserOptions,
};

/// Bounded AST walker that reports shell effects without executing source.
pub(in crate::security) struct ShellAstAnalyzer;

impl ShellAstAnalyzer {
    pub(in crate::security) fn analyze(source: &str) -> Effects {
        let mut effects = Effects::default();
        visit_source(source, &mut effects, 0);
        effects
    }
}

pub(super) fn visit_list(list: &ast::CompoundList, effects: &mut Effects, depth: usize) {
    for item in &list.0 {
        visit_pipeline(&item.0.first, effects, depth);
        for next in &item.0.additional {
            match next {
                ast::AndOr::And(pipeline) | ast::AndOr::Or(pipeline) => {
                    visit_pipeline(pipeline, effects, depth);
                }
            }
        }
    }
}

fn visit_pipeline(pipeline: &ast::Pipeline, effects: &mut Effects, depth: usize) {
    for command in &pipeline.seq {
        match command {
            ast::Command::Simple(simple) => visit_simple(simple, effects, depth),
            ast::Command::Compound(compound, redirects) => {
                visit_compound(compound, effects, depth);
                if let Some(redirects) = redirects {
                    for redirect in &redirects.0 {
                        visit_redirect(redirect, effects, depth);
                    }
                }
            }
            ast::Command::Function(function) => {
                // A definition changes shell state; its body may run later in this command.
                effects.raise(
                    RiskLevel::Mutating,
                    "shell-function",
                    "shell function definition",
                );
                visit_compound(&function.body.0, effects, depth);
            }
            ast::Command::ExtendedTest(_, redirects) => {
                if let Some(redirects) = redirects {
                    for redirect in &redirects.0 {
                        visit_redirect(redirect, effects, depth);
                    }
                }
            }
        }
    }
    if pipeline.seq.len() > 1
        && pipeline.seq.iter().skip(1).any(|command| {
            matches!(command, ast::Command::Simple(simple) if simple.word_or_name.as_ref().and_then(|word| static_word(&word.value)).is_some_and(|name| matches!(name.rsplit('/').next(), Some("sh" | "bash" | "ash" | "dash" | "mksh" | "su"))))
        })
    {
        effects.raise(
            RiskLevel::Dangerous,
            "shell-pipe-reentry",
            "shell evaluates pipeline input",
        );
    }
}

fn visit_compound(command: &ast::CompoundCommand, effects: &mut Effects, depth: usize) {
    match command {
        ast::CompoundCommand::BraceGroup(group) => visit_list(&group.list, effects, depth),
        ast::CompoundCommand::Subshell(group) => visit_list(&group.list, effects, depth),
        ast::CompoundCommand::IfClause(branch) => {
            visit_list(&branch.condition, effects, depth);
            visit_list(&branch.then, effects, depth);
            if let Some(elses) = &branch.elses {
                for branch in elses {
                    if let Some(condition) = &branch.condition {
                        visit_list(condition, effects, depth);
                    }
                    visit_list(&branch.body, effects, depth);
                }
            }
        }
        ast::CompoundCommand::WhileClause(loop_) | ast::CompoundCommand::UntilClause(loop_) => {
            visit_list(&loop_.0, effects, depth);
            visit_list(&loop_.1.list, effects, depth);
        }
        ast::CompoundCommand::ForClause(loop_) => {
            if let Some(values) = &loop_.values {
                for word in values {
                    visit_word(word, effects, depth);
                }
            }
            visit_list(&loop_.body.list, effects, depth);
        }
        ast::CompoundCommand::CaseClause(case_) => {
            visit_word(&case_.value, effects, depth);
            for branch in &case_.cases {
                if let Some(body) = &branch.cmd {
                    visit_list(body, effects, depth);
                }
            }
        }
        _ => effects.raise(
            RiskLevel::Dangerous,
            "shell-complex-unknown",
            "complex shell expression needs review",
        ),
    }
}

fn visit_simple(command: &ast::SimpleCommand, effects: &mut Effects, depth: usize) {
    let mut args = Vec::new();
    if let Some(prefix) = &command.prefix {
        for item in &prefix.0 {
            visit_item(item, effects, depth, &mut Vec::new());
        }
    }
    if let Some(name) = &command.word_or_name {
        visit_word(name, effects, depth);
        args.push(name.value.as_str());
    }
    if let Some(suffix) = &command.suffix {
        for item in &suffix.0 {
            visit_item(item, effects, depth, &mut args);
        }
    }
    let Some(name) = command.word_or_name.as_ref() else {
        return;
    };
    let Some(static_name) = static_word(&name.value) else {
        effects.raise(
            RiskLevel::Dangerous,
            "shell-dynamic-command",
            "dynamic command name",
        );
        return;
    };
    let mut basename = static_name
        .rsplit('/')
        .next()
        .unwrap_or(static_name.as_str());
    let mut arguments = args.iter().skip(1).copied().collect::<Vec<_>>();
    if basename == "command"
        && arguments
            .first()
            .is_some_and(|arg| matches!(*arg, "-v" | "-V"))
    {
        return;
    }
    let wrapped_name;
    if matches!(
        basename,
        "env" | "toybox" | "busybox" | "toolbox" | "command"
    ) {
        if let Some(index) = arguments
            .iter()
            .position(|arg| !arg.starts_with('-') && !arg.contains('='))
        {
            wrapped_name = static_word(arguments[index]);
            let Some(name) = &wrapped_name else {
                effects.raise(
                    RiskLevel::Dangerous,
                    "shell-wrapper-unknown",
                    "wrapped command is not statically known",
                );
                return;
            };
            basename = name.rsplit('/').next().unwrap_or(name);
            arguments = arguments[index + 1..].to_vec();
        } else {
            effects.raise(
                RiskLevel::Dangerous,
                "shell-wrapper-unknown",
                "wrapped command is not statically known",
            );
            return;
        }
    }
    let resolved = arguments
        .iter()
        .map(|arg| static_word(arg))
        .collect::<Vec<_>>();
    let static_args = resolved
        .iter()
        .map(|arg| arg.clone().unwrap_or_default())
        .collect::<Vec<_>>();
    policy::classify_command(basename, &static_args, effects);
    if resolved.iter().any(Option::is_none) && effects.risk >= RiskLevel::Mutating {
        effects.raise(
            RiskLevel::Dangerous,
            "shell-dynamic-target",
            "a state-changing command has a dynamic argument",
        );
    }
    match basename {
        "alias" | "unalias" | "trap" | "xargs" => effects.raise(
            RiskLevel::Dangerous,
            "shell-dynamic-dispatch",
            "shell command may change or invoke code dynamically",
        ),
        "eval" | "source" | "." => effects.raise(
            RiskLevel::Dangerous,
            "shell-dynamic-evaluation",
            "shell code is evaluated dynamically",
        ),
        "sh" | "bash" | "ash" | "dash" | "mksh" | "su" => {
            if let Some(position) = arguments.iter().position(|arg| shell_code_option(arg)) {
                match arguments
                    .get(position + 1)
                    .and_then(|code| static_word(code))
                {
                    Some(code) => visit_source(&code, effects, depth + 1),
                    None => effects.raise(
                        RiskLevel::Dangerous,
                        "shell-dynamic-reentry",
                        "shell code is not statically known",
                    ),
                }
            } else if arguments.iter().any(|arg| !arg.starts_with('-')) {
                effects.raise(
                    RiskLevel::Dangerous,
                    "shell-script-unknown",
                    "shell script content is not statically known",
                );
            } else if arguments.is_empty() {
                effects.raise(
                    RiskLevel::Dangerous,
                    "shell-interactive-unknown",
                    "interactive shell input is not statically known",
                );
            }
        }
        name if is_code_interpreter(name) => {
            effects.raise(
                RiskLevel::Dangerous,
                "interpreter-code-unknown",
                "non-shell interpreter code is not analyzed",
            );
        }
        _ => {}
    }
}

fn shell_code_option(argument: &str) -> bool {
    argument == "-c"
        || argument
            .strip_prefix('-')
            .is_some_and(|flags| !flags.starts_with('-') && flags.contains('c'))
}

fn is_code_interpreter(name: &str) -> bool {
    matches!(
        name,
        "perl" | "ruby" | "node" | "nodejs" | "lua" | "luajit" | "awk" | "gawk"
    ) || name.strip_prefix("python").is_some_and(|suffix| {
        suffix
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
    })
}

fn visit_item<'a>(item: &'a Item, effects: &mut Effects, depth: usize, args: &mut Vec<&'a str>) {
    match item {
        Item::IoRedirect(redirect) => visit_redirect(redirect, effects, depth),
        Item::Word(word) | Item::AssignmentWord(_, word) => {
            visit_word(word, effects, depth);
            args.push(&word.value);
        }
        Item::ProcessSubstitution(_, subshell) => visit_list(&subshell.list, effects, depth),
    }
}

fn visit_redirect(redirect: &ast::IoRedirect, effects: &mut Effects, depth: usize) {
    match redirect {
        ast::IoRedirect::File(_, kind, target) => match target {
            ast::IoFileRedirectTarget::Filename(word)
            | ast::IoFileRedirectTarget::Duplicate(word) => {
                visit_word(word, effects, depth);
                let fd_duplicate = matches!(kind, RedirectKind::DuplicateOutput)
                    && static_word(&word.value).is_some_and(|value| {
                        value == "-" || value.chars().all(|c| c.is_ascii_digit())
                    });
                if !matches!(kind, RedirectKind::Read | RedirectKind::DuplicateInput)
                    && !fd_duplicate
                    && static_word(&word.value).as_deref() != Some("/dev/null")
                {
                    if let Some(target) = static_word(&word.value) {
                        policy::classify_redirect(&target, effects);
                    }
                    effects.raise(
                        RiskLevel::Mutating,
                        "shell-output-redirect",
                        "shell output redirection writes a target",
                    );
                }
            }
            ast::IoFileRedirectTarget::ProcessSubstitution(_, subshell) => {
                visit_list(&subshell.list, effects, depth)
            }
            ast::IoFileRedirectTarget::Fd(_) => {}
        },
        ast::IoRedirect::OutputAndError(word, _) => {
            visit_word(word, effects, depth);
            if static_word(&word.value).as_deref() != Some("/dev/null") {
                if let Some(target) = static_word(&word.value) {
                    policy::classify_redirect(&target, effects);
                }
                effects.raise(
                    RiskLevel::Mutating,
                    "shell-output-redirect",
                    "shell output redirection writes a target",
                );
            }
        }
        ast::IoRedirect::HereString(_, word) => visit_word(word, effects, depth),
        ast::IoRedirect::HereDocument(_, document) => {
            if document.requires_expansion {
                match word::parse_heredoc(&document.doc.value, &ParserOptions::default()) {
                    Ok(pieces) => visit_pieces(&pieces, effects, depth),
                    Err(_) => effects.raise(
                        RiskLevel::Dangerous,
                        "shell-word-unknown",
                        "here-document could not be analyzed",
                    ),
                }
            }
        }
    }
}
