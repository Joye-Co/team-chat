import Foundation
import LocalAuthentication

/// Card answers (M2). The core queues them; the server's verdict arrives as `cardAnswered`
/// (tick `card:<message>`, a refusal in `cardRefusals`).
extension AppModel {
    func cardRecord(_ message: String) -> CardRecord? {
        (try? client?.cardRecord(messageId: message)) ?? nil
    }

    /// Answers a v2 card. `inputs` and `edits` are keyed by block id. False when it could not
    /// be queued.
    @discardableResult
    func answerCard(_ message: String, rev: UInt32, action: String, inputs: [String: Any], edits: [String: String]) -> Bool {
        guard let client else { return false }
        _ = try? client.forgetCardAnswer(messageId: message)
        let ok = (try? client.answerCard(
            messageId: message, rev: rev, actionId: action,
            inputsJson: AppModel.json(inputs), editsJson: AppModel.json(edits)
        )) != nil
        return ok
    }

    /// Commits one block of a 2.1 card (a yes/no `confirm`, an immediate choice).
    @discardableResult
    func commitCardBlock(_ message: String, rev: UInt32, block: String, event: String, value: Any) -> Bool {
        guard let client, let json = AppModel.json(value) else { return false }
        _ = try? client.forgetCardAnswer(messageId: message)
        let ok = (try? client.commitCardBlock(messageId: message, rev: rev, blockId: block, event: event, valueJson: json)) != nil
        return ok
    }

    /// Presses a button of a v1 card.
    @discardableResult
    func pressCard(_ message: String, action: String) -> Bool {
        guard let client else { return false }
        _ = try? client.forgetCardAnswer(messageId: message)
        let ok = (try? client.pressCard(messageId: message, actionId: action)) != nil
        return ok
    }

    /// Asks the owner of the phone to prove it is them (Face ID, else the passcode). A phone
    /// with neither set lets the confirmation sheet stand alone.
    nonisolated func confirmOwner(reason: String) async -> Bool {
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else {
            return (error as? LAError)?.code == .passcodeNotSet
        }
        return (try? await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)) ?? false
    }

    static func json(_ value: Any) -> String? {
        if let dict = value as? [String: Any], dict.isEmpty { return nil }
        if let dict = value as? [String: String], dict.isEmpty { return nil }
        guard JSONSerialization.isValidJSONObject([value]),
            let data = try? JSONSerialization.data(withJSONObject: [value], options: [.fragmentsAllowed]),
            let text = String(data: data, encoding: .utf8)
        else { return nil }
        // Strip the array we wrapped the value in (fragments like `true` are valid answers).
        return String(text.dropFirst().dropLast())
    }
}
