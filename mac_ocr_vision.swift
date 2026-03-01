import Foundation
import Vision

func runOCR(imagePath: String) -> String {
    let url = URL(fileURLWithPath: imagePath)
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = true
    request.automaticallyDetectsLanguage = true

    // Prefer Vietnamese/English when supported, while still allowing auto-detection.
    // This helps mixed UI screenshots where Vietnamese appears alongside English labels.
    do {
        let supported = try request.supportedRecognitionLanguages()
        let preferred = ["vi-VN", "en-US", "en-GB"]
        let chosen = preferred.filter { supported.contains($0) }
        if !chosen.isEmpty {
            request.recognitionLanguages = chosen
        }
    } catch {
        // Keep defaults if supported language lookup fails.
    }

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
