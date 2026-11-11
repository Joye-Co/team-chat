import Foundation
import LocalAuthentication
import SwiftUI

/// 账号 (M4): the other ways to sign in, my devices, my password and 面容解锁. Each call that
/// waits for the server runs off the main thread and answers what went wrong, if anything.
extension AppModel {
    // MARK: Signing in

    /// Runs a sign-in call off the main thread, then keeps the new token.
    private func finishSignIn(server: String, invalid: String, unauthorized: String, _ call: @escaping @Sendable () throws -> SignedIn) async -> String? {
        let result: Result<SignedIn, Error> = await Task.detached { Result { try call() } }.value
        switch result {
        case .success(let signed): return signIn(server: server, token: signed.token)
        case .failure(let error): return AppModel.accountMessage(for: error, invalid: invalid, unauthorized: unauthorized)
        }
    }

    func signIn(server: String, email: String, password: String, device: String) async -> String? {
        await finishSignIn(server: server, invalid: Copy.failEmail, unauthorized: Copy.passwordWrong) {
            try passwordSignIn(baseUrl: server, email: email, password: password, deviceName: device)
        }
    }

    func join(server: String, invite: String, device: String) async -> String? {
        await finishSignIn(server: server, invalid: Copy.inviteWrong, unauthorized: Copy.inviteWrong) {
            try redeemInvite(baseUrl: server, inviteCode: invite, deviceName: device)
        }
    }

    func claim(server: String, code: String, device: String) async -> String? {
        await finishSignIn(server: server, invalid: Copy.scanWrong, unauthorized: Copy.scanWrong) {
            try qrClaim(baseUrl: server, code: code, deviceName: device)
        }
    }

    /// A code for this phone to show; a signed-in computer approves it.
    nonisolated func askSignIn(server: String, device: String) async -> Result<SignInRequest, Error> {
        await Task.detached { Result { try qrRequest(baseUrl: server, deviceName: device) } }.value
    }

    /// Asks once whether the computer approved; signs in when it did.
    func pollAsk(server: String, request: SignInRequest) async -> SignInPoll? {
        let poll = await Task.detached { try? qrPoll(baseUrl: server, id: request.id, secret: request.secret) }.value
        if case .approved(let signed) = poll {
            _ = signIn(server: server, token: signed.token)
        }
        return poll
    }

    nonisolated static func accountMessage(for error: Error, invalid: String, unauthorized: String) -> String {
        switch error as? CoreError {
        case .Network: Copy.failNetwork
        case .Unauthorized, .NotFound: unauthorized
        case .RateLimited: Copy.failTooMany
        case .Invalid: invalid
        default: Copy.failOther
        }
    }

    // MARK: 我

    private func call<T>(_ work: @escaping @Sendable (ChatClient) throws -> T) async -> Result<T, Error> {
        guard let client else { return .failure(CoreError.Network) }
        return await Task.detached { Result { try work(client) } }.value
    }

    func devices() async -> Result<[DeviceItem], Error> {
        await call { try $0.devices() }
    }

    func signOutDevice(_ id: String) async -> String? {
        switch await call({ try $0.signOutDevice(id: id) }) {
        case .success: nil
        case .failure(let e): AppModel.message(for: e)
        }
    }

    func passwordState() async -> PasswordState? {
        try? await call { try $0.passwordState() }.get()
    }

    func signInEmail() async -> String? {
        (try? await call { try $0.myEmail() }.get()) ?? nil
    }

    /// Sets or changes my password. Returns what went wrong, if anything.
    func setPassword(_ password: String, current: String?) async -> String? {
        switch await call({ try $0.setPassword(password: password, current: current) }) {
        case .success: nil
        case .failure(let e): AppModel.accountMessage(for: e, invalid: current == nil ? Copy.passwordTooShort : Copy.currentPasswordWrong, unauthorized: Copy.signInAgain)
        }
    }

    func removePassword(current: String?) async -> String? {
        switch await call({ try $0.removePassword(current: current) }) {
        case .success: nil
        case .failure(let e): AppModel.accountMessage(for: e, invalid: Copy.currentPasswordWrong, unauthorized: Copy.signInAgain)
        }
    }

    /// The name of the device that shows `code`.
    func peekSignIn(_ code: String) async -> Result<String, AccountFailure> {
        switch await call({ try $0.peekSignIn(code: code) }) {
        case .success(let name): .success(name)
        case .failure(let e): .failure(AccountFailure(message: AppModel.accountMessage(for: e, invalid: Copy.scanWrong, unauthorized: Copy.scanWrong)))
        }
    }

    func approveSignIn(_ code: String) async -> String? {
        switch await call({ try $0.approveSignIn(code: code) }) {
        case .success: nil
        case .failure(let e): AppModel.accountMessage(for: e, invalid: Copy.scanWrong, unauthorized: Copy.scanWrong)
        }
    }

    // MARK: 面容解锁

    static let faceUnlockKey = "faceUnlock"

    /// Asks for Face ID (or the passcode). True when the owner confirmed.
    nonisolated static func authenticateOwner(reason: String) async -> Bool {
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else { return true }
        return (try? await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)) ?? false
    }
}

/// What went wrong, in words for the screen.
struct AccountFailure: Error {
    let message: String
}

/// The device name sent at sign in; it names this phone in 登录的设备.
enum DeviceName {
    @MainActor static var current: String { UIDevice.current.name }
}
