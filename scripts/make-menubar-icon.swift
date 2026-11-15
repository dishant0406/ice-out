// Draws the Iceout menu bar icon: the same circle with a crack as the app icon, in one colour.
// macOS tints it for light and dark menu bars (a "template" image), so only its shape matters.
// It is drawn 36 x 36 pixels, which is 18 x 18 points on a Retina screen.
// Usage: swift scripts/make-menubar-icon.swift assets/menubar-icon.png
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let size = 36
let context = CGContext(
    data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

// The shapes of the app icon (1024 design space, y counting down), scaled to fill the 36 pixels.
// The circle has radius 310 of 1024, so it is scaled up to leave only a small margin.
let scale = CGFloat(size) / 680.0
context.scaleBy(x: scale, y: scale)
context.translateBy(x: 340 - 512, y: 340 - 512)
context.translateBy(x: 0, y: 1024)
context.scaleBy(x: 1, y: -1)

context.setFillColor(CGColor(red: 0, green: 0, blue: 0, alpha: 1))
context.fillEllipse(in: CGRect(x: 512 - 310, y: 512 - 310, width: 620, height: 620))

// The crack is taken out of the circle, so it is see-through in the menu bar. It is a little wider
// than in the app icon, so it still reads at 18 points.
context.setBlendMode(.clear)
context.move(to: CGPoint(x: 396, y: 170))
context.addLine(to: CGPoint(x: 628, y: 170))
context.addLine(to: CGPoint(x: 512, y: 720))
context.closePath()
context.fillPath()

let image = context.makeImage()!
let url = URL(fileURLWithPath: CommandLine.arguments[1]) as CFURL
let destination = CGImageDestinationCreateWithURL(url, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(destination, image, nil)
CGImageDestinationFinalize(destination)
