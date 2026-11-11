import AVFoundation
import SwiftUI

/// The camera reading QR codes (AVFoundation). Calls `onCode` once per new text it reads;
/// `onProblem` when there is no camera or no permission.
struct CodeScanner: UIViewRepresentable {
    let onCode: (String) -> Void
    let onProblem: (String) -> Void

    /// False in the simulator and on a device without a back camera.
    static var available: Bool {
        #if targetEnvironment(simulator)
            false
        #else
            AVCaptureDevice.default(for: .video) != nil
        #endif
    }

    func makeCoordinator() -> Coordinator { Coordinator(onCode: onCode) }

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.backgroundColor = .black
        let coordinator = context.coordinator
        let problem = onProblem
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            coordinator.start(in: view, onProblem: problem)
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .video) { granted in
                DispatchQueue.main.async {
                    if granted { coordinator.start(in: view, onProblem: problem) } else { problem(Copy.scanCameraDenied) }
                }
            }
        default:
            DispatchQueue.main.async { problem(Copy.scanCameraDenied) }
        }
        return view
    }

    func updateUIView(_ uiView: PreviewView, context: Context) {}

    static func dismantleUIView(_ uiView: PreviewView, coordinator: Coordinator) {
        coordinator.stop()
    }

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        var preview: AVCaptureVideoPreviewLayer { layer as! AVCaptureVideoPreviewLayer }
    }

    final class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate {
        private let session = AVCaptureSession()
        private let onCode: (String) -> Void
        private var last: String?

        init(onCode: @escaping (String) -> Void) {
            self.onCode = onCode
        }

        func start(in view: PreviewView, onProblem: (String) -> Void) {
            guard let camera = AVCaptureDevice.default(for: .video),
                let input = try? AVCaptureDeviceInput(device: camera),
                session.canAddInput(input)
            else {
                onProblem(Copy.scanNoCamera)
                return
            }
            session.addInput(input)
            let output = AVCaptureMetadataOutput()
            guard session.canAddOutput(output) else {
                onProblem(Copy.scanNoCamera)
                return
            }
            session.addOutput(output)
            output.setMetadataObjectsDelegate(self, queue: .main)
            output.metadataObjectTypes = [.qr]
            view.preview.session = session
            view.preview.videoGravity = .resizeAspectFill
            let session = self.session
            DispatchQueue.global(qos: .userInitiated).async { session.startRunning() }
        }

        func stop() {
            let session = self.session
            DispatchQueue.global(qos: .userInitiated).async { session.stopRunning() }
        }

        func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject], from connection: AVCaptureConnection) {
            guard let text = (objects.first as? AVMetadataMachineReadableCodeObject)?.stringValue, text != last else { return }
            last = text
            UINotificationFeedbackGenerator().notificationOccurred(.success)
            onCode(text)
        }
    }
}
