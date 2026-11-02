//! The login view.

crate::copy! {
    TITLE = "登录",
    WELCOME = "欢迎加入团队",
    LEAD = "输入管理员发给你的邀请码",
    FOOTER = "版本 {version} · 登录后凭证保存在钥匙串里",
    SERVER_LABEL = "服务器",
    SERVER_PLACEHOLDER = "https://",
    CODE_LABEL = "邀请码",
    CODE_TOKEN = "已粘贴登录凭证",
    CHANGE_SERVER = "更改",
    CODE_PLACEHOLDER = "XXXX-XXXX-XXXX",
    CODE_HINT = "向管理员要一个邀请码。粘贴带横线或空格的格式也可以。",
    SUBMIT = "加入",
    SIGNING_IN = "正在加入…",
    ERROR_BAD_CREDENTIAL = "邀请码不对，请检查后再试。",
    ERROR_UNREACHABLE = "连不上服务器，请检查地址和网络",
    ERROR_UNSUPPORTED = "这个版本太旧了，请更新应用",
    ERROR_OTHER = "登录失败，请稍后再试",
    ERROR_EMPTY = "请填写服务器地址",
}
