use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "hangar",
    version,
    about = "Codex 多账号管理 CLI/TUI",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// 输出机器可读 JSON（仅一次性命令）
    #[arg(long, global = true)]
    pub json: bool,

    /// 跳过交互界面启动时的自动更新检查
    #[arg(long, global = true)]
    pub no_update: bool,

    /// 兼容旧版：强制经典菜单
    #[arg(long, hide = true)]
    pub classic: bool,

    /// 兼容旧版：等同于 `hangar update`
    #[arg(long, hide = true)]
    pub check_update: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// 列出账号（不输出任何凭据）
    List,
    /// 显示当前使用中的账号
    Current,
    /// 切换到完整 ID 或唯一邮箱指定的账号
    Switch { selector: String },
    /// 浏览器登录并添加账号（不自动切换）
    Login,
    /// 重新登录并原位复活指定账号
    Reauth { selector: String },
    /// 删除非当前账号；非交互安全要求显式 --yes
    Remove {
        selector: String,
        #[arg(long)]
        yes: bool,
    },
    /// 查询当前、指定或全部正常账号的配额
    Quota {
        selector: Option<String>,
        #[arg(long, conflicts_with = "selector")]
        all: bool,
    },
    /// 离线自检
    Doctor,
    /// 从官方 auth.json 收敛最新凭据
    Harvest,
    /// 显式检查并安装更新
    Update,
    /// 启动全屏 TUI
    Tui,
    /// 启动经典交互菜单
    Classic,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands_and_global_json() {
        let cli = Cli::try_parse_from(["hangar", "list", "--json"]).unwrap();
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Command::List)));

        let cli = Cli::try_parse_from(["hangar", "quota", "alice@example.com"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Quota {
                selector: Some(_),
                all: false
            })
        ));
    }

    #[test]
    fn rejects_unknown_or_conflicting_arguments() {
        assert!(Cli::try_parse_from(["hangar", "--wat"]).is_err());
        assert!(Cli::try_parse_from(["hangar", "quota", "alice@example.com", "--all"]).is_err());
    }
}
