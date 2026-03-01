import Foundation
import Vision

func runOCR(imagePath: String) -> String {
    let url = URL(fileURLWithPath: imagePath)
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = true

    let handler = VNImageRequestHandler(url: url, options: [:])
    do {
        try handler.perform([request])
    } catch {
        return ""
    }

    guard let observations = request.results as? [VNRecognizedTextObservation] else { return "" }
    let lines = observations.compactMap { $0.topCandidates(1).first?.string }
    return lines.joined(separator: " ")
}

let args = CommandLine.arguments
if args.count < 2 {
    print("")
    exit(1)
}

let text = runOCR(imagePath: args[1])
print(text)
