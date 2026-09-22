mod args;
mod classic;
mod commands;
mod output;
mod selector;
mod tui;
mod tui_overview;
mod ui;

use args::{Cli, Command};
use clap::{error::ErrorKind as ClapErrorKind, Parser};
use hangar_core as core;
use std::io::IsTerminal;

fn main() {
    let raw: Vec<_> = std::env::args_os().collect();
    let wants_json = raw.iter().any(|arg| arg == "--json");
    match Cli::try_parse_from(raw) {
        Ok(cli) => dispatch(cli),
        Err(error)
            if matches!(
                error.kind(),
                ClapErrorKind::DisplayHelp | ClapErrorKind::DisplayVersion
            ) =>
        {
            let exit_code = error.exit_code();
            let _ = error.print();
            std::process::exit(exit_code);
        }
        Err(error) if wants_json => {
            exit_error(commands::ErrorKind::Usage, &error.to_string(), true);
        }
        Err(error) => error.exit(),
    }
}

fn dispatch(cli: Cli) {
    if cli.check_update && cli.command.is_some() {
        exit_error(
            commands::ErrorKind::Usage,
            "--check-update 不能与子命令同时使用",
            cli.json,
        );
    }
    if cli.classic && cli.command.is_some() {
        exit_error(
            commands::ErrorKind::Usage,
            "--classic 不能与子命令同时使用",
            cli.json,
        );
    }

    let command = if cli.check_update {
        Some(Command::Update)
    } else if cli.classic {
        Some(Command::Classic)
    } else {
        cli.command
    };

    match command {
        Some(Command::Tui) => {
            if cli.json {
                exit_error(commands::ErrorKind::Usage, "tui 不支持 --json", true);
            }
            if !std::io::stdout().is_terminal() {
                exit_error(
                    commands::ErrorKind::Usage,
                    "tui 需要交互式终端；非 TTY 请使用一次性命令或 classic",
                    false,
                );
            }
            cleanup_and_maybe_update(cli.no_update);
            run_tui();
        }
        Some(Command::Classic) => {
            if cli.json {
                exit_error(commands::ErrorKind::Usage, "classic 不支持 --json", true);
            }
            cleanup_and_maybe_update(cli.no_update);
            classic_loop();
        }
        Some(command) => {
            core::updater::cleanup_pending_old();
            if let Err(error) = commands::execute(command, cli.json) {
                exit_error(error.kind, &error.message, cli.json);
            }
        }
        None => {
            if cli.json {
                exit_error(
                    commands::ErrorKind::Usage,
                    "--json 必须与一次性子命令一起使用",
                    true,
                );
            }
            cleanup_and_maybe_update(cli.no_update);
            if std::io::stdout().is_terminal() {
                run_tui();
            } else {
                classic_loop();
            }
        }
    }
}

fn exit_error(kind: commands::ErrorKind, message: &str, json: bool) -> ! {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({
                "ok": false,
                "code": kind as i32,
                "error": message,
            })
        );
    } else {
        eprintln!("{message}");
    }
    std::process::exit(kind as i32);
}

fn cleanup_and_maybe_update(skip_update: bool) {
    core::updater::cleanup_pending_old();
    let skip_update = skip_update
        || std::env::var("HANGAR_NO_UPDATE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    if skip_update {
        return;
    }
    match core::updater::check_update(false, env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_NAME")) {
        Ok(Some(info)) => match core::updater::apply_update(&info) {
            Ok(version) => {
                println!("已升级到 {version}，请重新运行 hangar 生效");
                std::process::exit(0);
            }
            Err(error) => eprintln!("自动更新失败：{error}（继续使用旧版）"),
        },
        Ok(None) => {}
        Err(error) => eprintln!("检查更新失败：{error}（继续启动）"),
    }
}

fn run_tui() {
    if let Err(error) = tui::run() {
        eprintln!("TUI 启动失败（{error}），回退经典模式");
        classic_loop();
    }
}

fn classic_loop() {
    ui::banner();
    loop {
        // 每次交互前重新收敛：长会话期间 Codex 可能轮换 refresh token。
        core::account::harvest();
        if let Err(error) = classic::show_menu_and_handle() {
            eprintln!("  {}", ui::error(&error));
        }
        println!();
    }
}
