import Foundation

final class HelperCommandHandler: NSObject, HelperToolProtocol, @unchecked Sendable {
    weak var connection: NSXPCConnection?

    func execute(script: String, instanceID: String, withReply reply: @escaping (Int32, String) -> Void) {
        execute(script: script, instanceID: instanceID, workingDirectory: "", withReply: reply)
    }

    func execute(script: String, instanceID: String, workingDirectory: String, withReply reply: @escaping (Int32, String) -> Void) {
        let proxy = connection?.remoteObjectProxy as? HelperProgressProtocol
        DaemonCore.execute(
            script: script,
            instanceID: instanceID,
            workingDirectory: workingDirectory,
            progressHandler: { proxy?.progressUpdate($0) },
            reply: reply
        )
    }

    func cancelOperation(instanceID: String, withReply reply: @escaping () -> Void) {
        DaemonCore.cancel(instanceID: instanceID)
        reply()
    }
}

final class HelperDelegate: NSObject, NSXPCListenerDelegate {
    func listener(_ listener: NSXPCListener, shouldAcceptNewConnection connection: NSXPCConnection) -> Bool {
        let handler = HelperCommandHandler()
        handler.connection = connection
        connection.exportedInterface = NSXPCInterface(with: HelperToolProtocol.self)
        connection.remoteObjectInterface = NSXPCInterface(with: HelperProgressProtocol.self)
        connection.exportedObject = handler
        connection.resume()
        return true
    }
}

let delegate = HelperDelegate()
let listener = NSXPCListener(machServiceName: "Agent.app.toddbruss.helper")
listener.delegate = delegate
listener.resume()
RunLoop.current.run()
