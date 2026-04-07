import Foundation

@objc protocol HelperToolProtocol {
    func execute(script: String, instanceID: String, withReply reply: @escaping (Int32, String) -> Void)
    func execute(script: String, instanceID: String, workingDirectory: String, withReply reply: @escaping (Int32, String) -> Void)
    func cancelOperation(instanceID: String, withReply reply: @escaping () -> Void)
}

@objc protocol HelperProgressProtocol {
    func progressUpdate(_ line: String)
}
