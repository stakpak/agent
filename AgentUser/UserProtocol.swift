import Foundation

@objc protocol UserToolProtocol {
    func execute(script: String, instanceID: String, withReply reply: @escaping (Int32, String) -> Void)
    func execute(script: String, instanceID: String, workingDirectory: String, withReply reply: @escaping (Int32, String) -> Void)
    func cancelOperation(instanceID: String, withReply reply: @escaping () -> Void)
}

@objc protocol UserProgressProtocol {
    func progressUpdate(_ line: String)
}
