import Foundation

/// Words of 账号 (M4): the ways to sign in, the password, signing other devices in, and 我.
extension Copy {
    // 登录: choose a way
    static let startTitle = "登录"
    static let startHint = "选一种方式进入团队。"
    static let wayEmail = "邮箱验证码"
    static let wayEmailHint = "发一个 6 位验证码到你的登录邮箱"
    static let wayPassword = "邮箱和密码"
    static let wayPasswordHint = "设过密码就可以直接登录"
    static let wayScan = "扫电脑上的码"
    static let wayScanHint = "在电脑的 设置 › 账号 里显示二维码"
    static let wayInvite = "邀请码"
    static let wayInviteHint = "第一次加入团队，用别人发给你的码"
    static let wayAsk = "让电脑帮我登录"
    static let wayAskHint = "手机上显示一个码，在电脑上输入"

    // 密码登录
    static let passwordTitle = "用密码登录"
    static let passwordHint = "用你的登录邮箱和设过的密码。"
    static let passwordField = "密码"
    static let passwordSubmit = "登录"
    static let passwordWrong = "邮箱或密码不对。"
    static let passwordShow = "显示密码"
    static let passwordHide = "隐藏密码"
    static let passwordForgot = "忘了密码？用邮箱验证码登录"

    // 邀请码
    static let inviteTitle = "用邀请码加入"
    static let inviteHint = "邀请码有 8 位，像 ABCD-EFGH，7 天内有效。"
    static let inviteField = "邀请码"
    static let inviteSubmit = "加入"
    static let inviteWrong = "邀请码不对，或者已经用过、过期了。"

    // 扫码
    static let scanTitle = "扫电脑上的码"
    static let scanHint = "在电脑上打开 设置 › 账号 › 在手机上登录，把二维码放进框里。"
    static let scanPaste = "粘贴码"
    static let scanTypeCode = "手动输入码"
    static let scanCodeField = "8 位码"
    static let scanSubmit = "登录"
    static let scanNoCamera = "这台设备用不了相机，请手动输入码。"
    static let scanCameraDenied = "没有相机权限。可以在系统设置里打开，或者手动输入码。"
    static let scanNotACode = "这不是登录用的码。"
    static let scanWrong = "码不对，或者已经用过、过期了。"
    static let scanSigningIn = "正在登录…"

    // 让电脑帮我登录
    static let askTitle = "让电脑帮我登录"
    static let askHint = "在已经登录的电脑上打开 设置 › 账号 › 在手机上登录，点「输入」，填这个码。"
    static let askWaiting = "等电脑确认…"
    static let askExpired = "码过期了。"
    static let askRenew = "换一个码"
    static let askCopy = "复制码"

    // 设置密码
    static let setPasswordTitle = "设置密码"
    static let changePasswordTitle = "修改密码"
    static let setPasswordHint = "设好以后，可以用登录邮箱和密码直接登录。至少 8 位。"
    static let currentPassword = "现在的密码"
    static let newPassword = "新密码"
    static let repeatPassword = "再输一次"
    static let passwordsDiffer = "两次输入的密码不一样。"
    static let passwordTooShort = "至少 8 位。"
    static let currentPasswordWrong = "现在的密码不对。"
    static let save = "保存"
    static let cancel = "取消"
    static let strengthWeak = "太简单"
    static let strengthFair = "一般"
    static let strengthGood = "不错"
    static let strengthStrong = "很好"
    static let removePassword = "不用密码了"
    static let removePasswordConfirm = "去掉密码后，只能用邮箱验证码或扫码登录。"
    static let passwordNeedsEmail = "还没有登录邮箱。先在电脑的 设置 › 账号 里添加登录邮箱，密码才能用来登录。"

    // 我
    static let meDevices = "登录的设备"
    static let meDevicesHint = "你的账号在这些设备上登录着。让一台退出，它会马上失效。"
    static let meThisDevice = "这台设备"
    static let meUnnamedDevice = "没有名字的设备"
    static func meDeviceUsed(_ when: String) -> String { "\(when)用过" }
    static let meNeverUsed = "登录后还没用过"
    static let meSignOutDevice = "退出登录"
    static func meSignOutDeviceTitle(_ name: String) -> String { "让「\(name)」退出登录？" }
    static let meSignOutDeviceBody = "它会马上退出，要重新登录才能用。"
    static let meDevicesFailed = "没有加载出登录的设备。"
    static let meSecurity = "登录和安全"
    static let mePassword = "密码"
    static let mePasswordSet = "已设置"
    static let mePasswordNone = "没有设置"
    static let meEmail = "登录邮箱"
    static let meEmailNone = "还没有添加"
    static let meFaceUnlock = "打开时用面容解锁"
    static let meHelpSignIn = "帮另一台设备登录"
    static let meHelpSignInHint = "扫另一台设备上显示的码"
    static let meSignOutTitle = "退出这台设备的登录？"
    static let meSignOutBody = "这台设备上的缓存会删掉，其他设备不受影响。"
    static let meNotificationsSlot = "通知"

    // 帮另一台设备登录
    static let approveTitle = "让这台设备登录？"
    static func approveBody(_ name: String) -> String { "「\(name)」会登录你的账号，能看到你能看到的一切。不是你本人的设备请不要允许。" }
    static let approveAllow = "允许登录"
    static func approveDone(_ name: String) -> String { "「\(name)」已经登录。" }
    static let done = "完成"

    // 面容解锁
    static let unlockTitle = "已锁定"
    static let unlockButton = "解锁"
    static let unlockReason = "解锁后查看团队的消息"
}
