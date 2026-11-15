// Draws the Iceout app icon (the same shapes as assets/iceout-icon.svg) into a PNG, on the
// standard macOS icon grid: an 824 x 824 rounded square centred in a 1024 x 1024 canvas.
// Usage: swift scripts/make-icon.swift out.png
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let size = 1024
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let context = CGContext(
    data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
    space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

func color(_ hex: UInt32) -> CGColor {
    CGColor(red: CGFloat((hex >> 16) & 255) / 255, green: CGFloat((hex >> 8) & 255) / 255, blue: CGFloat(hex & 255) / 255, alpha: 1)
}

// The brand gradient: electric cyan, blue, violet, hot pink, from the top left to the bottom right.
let gradient = CGGradient(
    colorsSpace: space,
    colors: [color(0x19E3FF), color(0x3D7BFF), color(0x7B4DFF), color(0xFF4DC8)] as CFArray,
    locations: [0, 0.38, 0.7, 1])!

// Shapes are drawn in the 1024 design space of the SVG, then scaled onto the 824 body.
context.translateBy(x: 100, y: 100)
context.scaleBy(x: 824.0 / 1024.0, y: 824.0 / 1024.0)
// The SVG has y counting down; Core Graphics counts up. Flip.
context.translateBy(x: 0, y: 1024)
context.scaleBy(x: 1, y: -1)

func paintGradient() {
    context.drawLinearGradient(gradient, start: CGPoint(x: 0, y: 0), end: CGPoint(x: 1024, y: 1024), options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
}

// The rounded square, filled with the gradient.
context.saveGState()
context.addPath(CGPath(roundedRect: CGRect(x: 0, y: 0, width: 1024, height: 1024), cornerWidth: 230, cornerHeight: 230, transform: nil))
context.clip()
paintGradient()
// A soft light from the top left, so the colour has depth.
let light = CGGradient(colorsSpace: space, colors: [CGColor(red: 1, green: 1, blue: 1, alpha: 0.32), CGColor(red: 1, green: 1, blue: 1, alpha: 0)] as CFArray, locations: [0, 1])!
context.drawRadialGradient(light, startCenter: CGPoint(x: 220, y: 160), startRadius: 0, endCenter: CGPoint(x: 220, y: 160), endRadius: 640, options: [])
context.restoreGState()

// The white circle, with a wedge cut out of it (like a crack in ice) that shows the gradient.
context.setFillColor(color(0xFFFFFF))
context.fillEllipse(in: CGRect(x: 512 - 310, y: 512 - 310, width: 620, height: 620))

context.saveGState()
context.move(to: CGPoint(x: 424, y: 170))
context.addLine(to: CGPoint(x: 600, y: 170))
context.addLine(to: CGPoint(x: 512, y: 700))
context.closePath()
context.clip()
paintGradient()
context.drawRadialGradient(light, startCenter: CGPoint(x: 220, y: 160), startRadius: 0, endCenter: CGPoint(x: 220, y: 160), endRadius: 640, options: [])
context.restoreGState()

let image = context.makeImage()!
let url = URL(fileURLWithPath: CommandLine.arguments[1]) as CFURL
let destination = CGImageDestinationCreateWithURL(url, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(destination, image, nil)
CGImageDestinationFinalize(destination)
