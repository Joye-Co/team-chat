//! The login view.

crate::copy! {
    TITLE = "登录",
    WELCOME = "欢迎使用 {product}",
    LEAD = "工作在话题里推进，成员一起完成。",
    FOOTER = "版本 {version} · 登录后凭证保存在钥匙串里",
    SERVER_LABEL = "服务器地址",
    SERVER_PLACEHOLDER = "https://",
    CODE_LABEL = "邀请码或登录凭证",
    CODE_PLACEHOLDER = "XXXX-XXXX-XXXX",
    CODE_HINT = "向管理员要一个邀请码，邀请码只能用一次",
    SUBMIT = "登录",
    SIGNING_IN = "正在登录…",
    ERROR_BAD_CREDENTIAL = "邀请码或登录凭证无效，可能已用过或已过期",
    ERROR_UNREACHABLE = "连不上服务器，请检查地址和网络",
    ERROR_UNSUPPORTED = "这个版本太旧了，请更新应用",
    ERROR_OTHER = "登录失败，请稍后再试",
    ERROR_EMPTY = "请填写服务器地址和邀请码",
}
