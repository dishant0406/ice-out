import AppKit
import ApplicationServices
func str(_ v: AnyObject?) -> String { v.map { "\($0)" } ?? "-" }
var out = "axTrusted: \(AXIsProcessTrusted())\n"
func attr(_ e: AXUIElement, _ a: String) -> AnyObject? {
  var v: CFTypeRef?
  return AXUIElementCopyAttributeValue(e, a as CFString, &v) == .success ? v : nil
}
for app in NSWorkspace.shared.runningApplications {
  let el = AXUIElementCreateApplication(app.processIdentifier)
  guard let bar = attr(el, "AXExtrasMenuBar") else { continue }
  let kids = (attr(bar as! AXUIElement, kAXChildrenAttribute as String) as? [AXUIElement]) ?? []
  out += "\(app.localizedName ?? "?") (\(app.bundleIdentifier ?? "?")) extras=\(kids.count)\n"
  for k in kids {
    let pos = attr(k, kAXPositionAttribute as String)
    var p = CGPoint.zero
    if let pos { AXValueGetValue(pos as! AXValue, .cgPoint, &p) }
    out += "   title=\(str(attr(k, kAXTitleAttribute as String))) desc=\(str(attr(k, kAXDescriptionAttribute as String))) id=\(str(attr(k, kAXIdentifierAttribute as String))) x=\(Int(p.x))\n"
  }
}
try? out.write(toFile: CommandLine.arguments[1], atomically: true, encoding: .utf8)
