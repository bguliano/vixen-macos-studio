import AppKit
import QuartzCore
import VixenBridge
import VixenKit

/// What scripts can ask of this window beyond clicks, by name, through the harness's `hook` action. A window a test runs is never key, so a mouse click doesn't reach its tables, and
/// measuring needs to get under the event loop.
enum SetupTestHooks {
    private static func summary(_ times: [Double]) -> String {
        guard !times.isEmpty else { return "no samples" }
        let sorted = times.sorted()
        let ms = { (t: Double) in String(format: "%.2f", t * 1000) }
        return "n=\(times.count) mean=\(ms(times.reduce(0, +) / Double(times.count)))ms p50=\(ms(sorted[sorted.count / 2]))ms p95=\(ms(sorted[sorted.count * 95 / 100]))ms max=\(ms(sorted.last ?? 0))ms"
    }

    /// Waits for background work to come back to the main thread.
    private static func settle(_ milliseconds: Int = 60) async {
        try? await Task.sleep(for: .milliseconds(milliseconds))
    }

    @MainActor
    static func install(model: SetupModel) {
        var hooks = UITestHarness.hooks

        // Waits for the session and the trees to be ready, and says how long opening took.
        hooks["setup.ready"] = { _ in
            let started = CACurrentMediaTime()
            while model.session == nil || model.elementTree.rowCount == 0 {
                try? await Task.sleep(for: .milliseconds(2))
                if CACurrentMediaTime() - started > 30 { return "FAIL session not ready" }
            }
            return String(format: "ready after %.1f ms, %d root rows", (CACurrentMediaTime() - started) * 1000, model.elementTree.rowCount)
        }

        // Selects element rows by name, in the order given, following first parents.
        hooks["setup.selectElements"] = { names in
            let rows = names.compactMap { name in elementRow(named: name, model: model) }
            guard rows.count == names.count else { return "FAIL no row for one of \(names)" }
            await model.elementTree.select(rows, expanding: ancestors(of: rows))
            await settle()
            return "\(model.elementCount) selected"
        }

        // A range gesture: the first name is the anchor and the second the endpoint.
        hooks["setup.rangeElements"] = { names in
            guard names.count == 2, let a = elementRow(named: names[0], model: model), let b = elementRow(named: names[1], model: model) else { return "FAIL needs two rows" }
            await model.elementTree.expand(ancestors(of: [a, b]))
            model.elementTree.selectRange(anchor: a, endpoint: b)
            await settle()
            return model.elementSelection.joined(separator: ",")
        }

        hooks["setup.selectionNames"] = { _ in
            model.session?.namesOf(selection: model.elementSelection).joined(separator: "|") ?? "FAIL"
        }

        hooks["setup.expandElements"] = { names in
            let rows = names.compactMap { name in elementRow(named: name, model: model) }
            await model.elementTree.expand(ancestors(of: rows) + rows)
            await settle()
            return "\(model.elementTree.rowCount) rows"
        }

        hooks["setup.selectControllers"] = { ids in
            let parents = ids.compactMap { $0.hasPrefix("o") ? "c" + $0.dropFirst().prefix { $0 != ":" } : nil }
            await model.controllerTree.select(ids, expanding: Array(Set(parents)))
            await settle()
            return "\(model.controllerRowCount) selected"
        }

        hooks["setup.expandControllers"] = { ids in
            await model.controllerTree.expand(ids)
            await settle()
            return "\(model.controllerTree.rowCount) rows"
        }

        hooks["setup.stats"] = { _ in
            await settle()
            let e = model.elementStats
            let c = model.controllerStats
            return "total \(e.total) connected \(e.connected) unconnected \(e.unconnected) items \(e.items) groups \(e.groups) elements \(e.leaves) filters \(e.filters) | outputs \(c.outputs) patched \(c.patched) unpatched \(c.unpatched) controllers \(c.controllers) first \(c.firstLabel) last \(c.lastLabel) | \(model.summary.text) | \(model.summary.warning) | \(model.summary.canPatch)"
        }

        hooks["setup.properties"] = { _ in
            await settle()
            return model.properties.map(\.typeName).joined(separator: ",")
        }

        // The list loads in the background after a selection, and arriving clears the property selection, so wait for it.
        hooks["setup.selectProperty"] = { names in
            for _ in 0..<500 where model.propertiesLoading || !names.allSatisfy({ name in model.properties.contains { $0.typeName == name } }) {
                try? await Task.sleep(for: .milliseconds(10))
            }
            model.propertySelection = Set(model.properties.filter { names.contains($0.typeName) }.map(\.typeId))
            return "\(model.propertySelection.count)"
        }

        hooks["setup.options"] = { flags in
            model.usesAllPoints = flags.contains("allPoints")
            model.usesAllOutputs = flags.contains("allOutputs")
            model.reversesElements = flags.contains("reverseElements")
            model.reversesOutputs = flags.contains("reverseOutputs")
            return "ok"
        }

        hooks["setup.dirty"] = { _ in
            model.session?.isDirty() == true ? "dirty" : "clean"
        }

        // Lists a tree's context menu as it stands: title, then "off" if disabled.
        hooks["setup.elementMenu"] = { _ in
            guard let menu = model.elementMenu() else { return "FAIL no menu" }
            return menuText(menu)
        }

        hooks["setup.controllerMenu"] = { _ in
            guard let menu = model.controllerMenu() else { return "no menu" }
            return menuText(menu)
        }

        // Runs an item of a tree's context menu by its path, separated by ">".
        hooks["setup.elementMenuRun"] = { path in
            guard let menu = model.elementMenu() else { return "FAIL no menu" }
            return run(menu, path)
        }

        hooks["setup.controllerMenuRun"] = { path in
            guard let menu = model.controllerMenu() else { return "FAIL no menu" }
            return run(menu, path)
        }

        hooks["setup.command"] = { names in
            switch names.first {
            case "addTemplate": Task { await model.addFromTemplate() }
            case "delete": Task { await model.deleteElements() }
            case "rename": Task { await model.renameElements() }
            case "findOutputs": Task { await model.findOutputs() }
            case "findElements": Task { await model.findElements() }
            case "runHelper": Task { await model.runHelper() }
            case "addProperty": Task { await model.addProperty() }
            case "deleteProperty": Task { await model.deleteProperties() }
            case "configureProperty": Task { await model.configureProperty() }
            case "patch": Task { await model.patchElements() }
            case "unpatchElements": Task { await model.unpatchElements() }
            case "unpatchControllers": Task { await model.unpatchControllers() }
            case "addController": Task { await model.addController() }
            case "moveUp": Task { await model.moveControllers(up: true) }
            case "moveDown": Task { await model.moveControllers(up: false) }
            case "configureController": Task { await model.configureController() }
            case "channelCount": Task { await model.channelCount() }
            case "renameController": Task { await model.renameController() }
            case "deleteController": Task { await model.deleteControllers() }
            case "start": Task { await model.startControllers() }
            case "stop": Task { await model.stopControllers() }
            case "ok": model.ok()
            case "cancel": model.cancel()
            case "help": model.help()
            case "graphical": model.usesGraphicalView = true
            case "simple": model.usesGraphicalView = false
            case "template": model.templateName = names.count > 1 ? names[1] : model.templateName
            case "helper": model.helperName = names.count > 1 ? names[1] : model.helperName
            case "controllerType":
                if let type = model.controllerTypes.first(where: { names.count > 1 && $0.name == names[1] }) {
                    model.controllerTypeID = type.id
                }
            default:
                return "FAIL no command \(names)"
            }
            await settle(150)
            return "ran"
        }

        // Answers the alert showing now, by the verb on its button.
        hooks["setup.alert"] = { verb in
            for _ in 0..<200 where model.alert == nil {
                try? await Task.sleep(for: .milliseconds(5))
            }
            guard let alert = model.alert else { return "FAIL no alert" }
            let text = "\(alert.title) / \(alert.message)"
            if let wanted = verb.first, let choice = alert.choices.first(where: { $0.title == wanted }) {
                model.alertAnswered(alert, choice.value)
                await settle(150)
            }
            return text
        }

        hooks["setup.sheetOpen"] = { _ in
            model.sheet != nil ? "open" : "none"
        }

        // Answers the prompt showing now: a value, or "cancel". A list takes an item's name, and its first item without one. A dialog's placeholder is dismissed.
        hooks["setup.sheetAnswer"] = { args in
            for _ in 0..<200 where model.sheet == nil {
                try? await Task.sleep(for: .milliseconds(5))
            }
            guard let sheet = model.sheet else { return "FAIL no sheet" }
            // As a person would, once it's on screen.
            try? await Task.sleep(for: .milliseconds(400))
            let cancel = args.first == "cancel"
            let text: String
            switch sheet.kind {
            case .text(let title, let prompt, let initial, _, let finish):
                text = "text \(title) / \(prompt) / \(initial)"
                finish(cancel ? nil : (args.first ?? initial))
            case .number(let title, let prompt, let value, let min, let max, let finish):
                text = "number \(title) / \(prompt) / \(value) in \(min)...\(max)"
                finish(cancel ? nil : (args.first.flatMap { Int($0) } ?? value))
            case .list(let title, let items, let finish):
                text = "list \(title) / \(items.map(\.name).joined(separator: ","))"
                finish(cancel ? nil : (items.first { $0.name == args.first } ?? items.first))
            case .dialog(let request, let finish):
                text = "dialog \(request)"
                finish(false)
            default:
                return "FAIL this kind of sheet isn't answered here"
            }
            await settle(150)
            return text
        }

        // Drop rows on a parent, the same call a drag ends in.
        hooks["setup.drop"] = { args in
            // sources..., "|", parent or "-", index or "on", "copy" or "move"
            guard let bar = args.firstIndex(of: "|"), args.count >= bar + 4 else { return "FAIL bad arguments" }
            let sources = args[..<bar].compactMap { elementRow(named: $0, model: model) }
            let parent = args[bar + 1] == "-" ? nil : elementRow(named: args[bar + 1], model: model)
            let index = Int(args[bar + 2])
            let copy = args[bar + 3] == "copy"
            let allowed = model.canDrop(items: sources, parent: parent, index: index, copy: copy)
            guard allowed else { return "refused" }
            await model.drop(items: sources, parent: parent, index: index, copy: copy)
            await settle()
            return "dropped"
        }

        // The canvas.
        hooks["setup.graph"] = { _ in
            await settle(120)
            let c = model.graph.canvas.counts
            return "\(c.shapes) shapes \(c.lines) lines zoom \(model.graph.toolbar.zoomPercent)%"
        }

        // The tree's rows on screen now, as "name" per row, so a script can check order.
        hooks["setup.rootNames"] = { _ in
            model.session?.elementRows(parent: nil).map(\.name).joined(separator: "|") ?? "FAIL"
        }

        hooks["setup.childNames"] = { names in
            guard let row = names.first.flatMap({ elementRow(named: $0, model: model) }) else { return "FAIL" }
            return model.session?.elementRows(parent: row).map(\.name).joined(separator: "|") ?? "FAIL"
        }

        // The controller tree's selection as the model holds it, and which of it a collapse has hidden.
        hooks["setup.controllerSelection"] = { _ in
            await settle()
            return "selected \(model.controllerSelection.joined(separator: ",")) | hidden \(model.controllerHidden.sorted().joined(separator: ",")) | highlighted \(model.controllerHighlighted.joined(separator: ","))"
        }

        hooks["setup.collapseControllers"] = { _ in
            model.controllerTree.collapseAll()
            await settle()
            return "\(model.controllerTree.rowCount) rows"
        }

        // Drops controller rows at a root index, the same call a drag ends in.
        hooks["setup.dropControllers"] = { args in
            guard let index = args.last.flatMap({ Int($0) }) else { return "FAIL" }
            await model.dropControllers(items: Array(args.dropLast()), index: index)
            await settle()
            return model.session?.controllerRows().map(\.title).joined(separator: "|") ?? "FAIL"
        }

        hooks["setup.controllerNames"] = { _ in
            model.session?.controllerRows().map(\.title).joined(separator: "|") ?? "FAIL"
        }

        // MARK: Measuring

        // Closes the window with Cancel and opens it again, timing from the open to the trees being filled, and the longest the main thread went unanswered meanwhile.
        hooks["setup.perf.reopen"] = { _ in
            model.cancel()
            for _ in 0..<400 where model.session != nil {
                try? await Task.sleep(for: .milliseconds(5))
            }
            try? await Task.sleep(for: .milliseconds(300))
            let t0 = CACurrentMediaTime()
            model.openSetupWindow?()
            var longest = 0.0
            var last = CACurrentMediaTime()
            while model.session == nil || model.elementTree.rowCount == 0 || model.controllerTree.rowCount == 0 {
                try? await Task.sleep(for: .milliseconds(1))
                let now = CACurrentMediaTime()
                longest = max(longest, now - last)
                last = now
                if now - t0 > 60 { return "FAIL not ready" }
            }
            let ready = CACurrentMediaTime()
            // And the first statistics, which follow off the main thread.
            for _ in 0..<200 {
                try? await Task.sleep(for: .milliseconds(1))
                let now = CACurrentMediaTime()
                longest = max(longest, now - last)
                last = now
            }
            return String(format: "open to trees filled %.1f ms (from windowOpened %.1f ms), longest main thread gap %.1f ms", (ready - t0) * 1000, (ready - model.openedAt) * 1000, longest * 1000)
        }

        // The longest main-thread gap while a selection of the named elements is made, which in the graphical view builds a scene off the main thread.
        hooks["setup.perf.selectGap"] = { names in
            let rows = names.compactMap { name in elementRow(named: name, model: model) }
            var longest = 0.0
            var last = CACurrentMediaTime()
            let t0 = last
            let before = model.graph.canvas.counts
            await model.elementTree.select(rows)
            while CACurrentMediaTime() - t0 < 3 {
                try? await Task.sleep(for: .milliseconds(1))
                let now = CACurrentMediaTime()
                longest = max(longest, now - last)
                last = now
                if model.graph.canvas.counts != before, now - t0 > 0.05 { break }
            }
            return String(format: "longest main thread gap %.1f ms; canvas %d shapes %d lines after %.1f ms", longest * 1000, model.graph.canvas.counts.shapes, model.graph.canvas.counts.lines, (CACurrentMediaTime() - t0) * 1000)
        }

        // Scrolls a tree from top to bottom, one screenful-fraction at a time, timing each display pass.
        hooks["setup.perf.scroll"] = { args in
            let handle = args.first == "controllers" ? model.controllerTree : model.elementTree
            guard let scroll = handle.scrollView, let document = scroll.documentView else { return "FAIL no tree" }
            let steps = Int(args.dropFirst().first ?? "") ?? 300
            let clip = scroll.contentView
            let travel = max(document.bounds.height - clip.bounds.height, 0)
            var times: [Double] = []
            for i in 0...steps {
                let t0 = CACurrentMediaTime()
                clip.scroll(to: NSPoint(x: 0, y: travel * CGFloat(i) / CGFloat(steps)))
                scroll.reflectScrolledClipView(clip)
                scroll.displayIfNeeded()
                CATransaction.flush()
                times.append(CACurrentMediaTime() - t0)
            }
            clip.scroll(to: .zero)
            scroll.reflectScrolledClipView(clip)
            return "rows \(handle.rowCount) travel \(Int(travel))pt " + summary(times)
        }

        // Selects a growing block of rows, as a held ⇧↓ does, timing the main thread's part. Each selection change also starts a stats refresh off the main thread.
        hooks["setup.perf.select"] = { args in
            let handle = args.first == "controllers" ? model.controllerTree : model.elementTree
            let rows = handle.rowCount
            let steps = min(Int(args.dropFirst().first ?? "") ?? 300, rows)
            var times: [Double] = []
            for i in 0..<steps {
                let t0 = CACurrentMediaTime()
                handle.selectRows(IndexSet(integersIn: 0...i))
                _ = handle.selection
                await Task.yield()
                times.append(CACurrentMediaTime() - t0)
            }
            await settle(100)
            return "rows \(rows) " + summary(times)
        }

        // Selects everything in one go, and how long until the numbers on screen are right.
        hooks["setup.perf.selectAll"] = { args in
            let handle = args.first == "controllers" ? model.controllerTree : model.elementTree
            let rows = handle.rowCount
            let t0 = CACurrentMediaTime()
            handle.selectRows(IndexSet(integersIn: 0..<rows))
            let main = CACurrentMediaTime() - t0
            let before = model.elementStats.items
            let wanted = args.first == "controllers" ? -1 : rows
            while wanted >= 0, model.elementStats.items != UInt32(wanted), CACurrentMediaTime() - t0 < 10 {
                try? await Task.sleep(for: .milliseconds(1))
            }
            return String(format: "rows %d select call %.2f ms, stats on screen after %.1f ms (items %d -> %d)", rows, main * 1000, (CACurrentMediaTime() - t0) * 1000, before, model.elementStats.items)
        }

        // How long the main thread can stall while something heavy runs: the longest gap a 1 ms timer sees during the named command.
        hooks["setup.perf.gap"] = { args in
            let seconds = Double(args.first ?? "") ?? 1
            var longest = 0.0
            var last = CACurrentMediaTime()
            let end = last + seconds
            while CACurrentMediaTime() < end {
                try? await Task.sleep(for: .milliseconds(1))
                let now = CACurrentMediaTime()
                longest = max(longest, now - last)
                last = now
            }
            return String(format: "longest main thread gap %.1f ms over %.1f s", longest * 1000, seconds)
        }

        // Expands a group whose name is given, timing it to the rows being there.
        hooks["setup.perf.expand"] = { names in
            guard let row = names.first.flatMap({ elementRow(named: $0, model: model) }) else { return "FAIL no row" }
            let before = model.elementTree.rowCount
            let t0 = CACurrentMediaTime()
            await model.elementTree.expand([row])
            let t1 = CACurrentMediaTime()
            model.elementTree.scrollView?.displayIfNeeded()
            return String(format: "%d -> %d rows in %.1f ms, first display %.1f ms", before, model.elementTree.rowCount, (t1 - t0) * 1000, (CACurrentMediaTime() - t1) * 1000)
        }

        // The graphical canvas: a full redraw, a filter drag and a connect drag, each timed per move on the main thread.
        hooks["setup.perf.graph"] = { args in
            let steps = Int(args.first ?? "") ?? 120
            let canvas = model.graph.canvas
            await settle(150)
            let c = canvas.counts
            var redraws: [Double] = []
            for _ in 0..<5 {
                redraws.append(canvas.measureFullRedraw())
            }
            var full: [Double] = []
            for _ in 0..<5 {
                full.append(canvas.measureVisibleDraw())
            }
            var report = "shapes \(c.shapes) lines \(c.lines) zoom \(model.graph.toolbar.zoomPercent)% redraw " + summary(redraws) + " | visible area drawn offscreen " + summary(full)
            if let drag = canvas.measureDrag(steps: steps) {
                report += " | filter drag " + summary(drag.moves) + String(format: " commit %.2fms", drag.redraw * 1000)
            }
            await settle(150)
            if let hover = canvas.measureConnectHover(steps: steps) {
                report += " | connect drag " + summary(hover)
            }
            return report
        }

        // From a selection change to the new picture on the canvas.
        hooks["setup.perf.scene"] = { names in
            let rows = names.compactMap { name in elementRow(named: name, model: model) }
            let t0 = CACurrentMediaTime()
            await model.elementTree.select(rows)
            let previous = model.graph.canvas.counts
            while CACurrentMediaTime() - t0 < 10 {
                try? await Task.sleep(for: .milliseconds(1))
                let now = model.graph.canvas.counts
                if now != previous { break }
            }
            return String(format: "scene on canvas after %.1f ms: %d shapes %d lines", (CACurrentMediaTime() - t0) * 1000, model.graph.canvas.counts.shapes, model.graph.canvas.counts.lines)
        }

        // MARK: Gestures on the canvas. Shapes go by title, with "#n" for which of that title.

        // "Title", "Title#2" for the third of that title, a trailing "^" for a group's header, or "@x,y" for whatever is on top there.
        func shape(_ name: String) -> (index: Int, point: CGPoint)? {
            let canvas = model.graph.canvas
            var spec = name
            let header = spec.hasSuffix("^")
            if header { spec.removeLast() }
            if spec.hasPrefix("@") {
                let n = spec.dropFirst().split(separator: ",").compactMap { Double($0) }
                guard n.count == 2 else { return nil }
                let p = CGPoint(x: n[0], y: n[1])
                return canvas.topShapeIndex(at: p).map { ($0, p) }
            }
            let parts = spec.split(separator: "#", omittingEmptySubsequences: false).map(String.init)
            guard let i = canvas.shapeIndex(titled: parts[0], occurrence: parts.count > 1 ? Int(parts[1]) ?? 0 : 0) else { return nil }
            let rect = canvas.shapeRect(i)
            return (i, header ? CGPoint(x: rect.midX, y: rect.minY + 10) : canvas.shapeCentre(i))
        }

        func flags(_ names: ArraySlice<String>) -> NSEvent.ModifierFlags {
            var out: NSEvent.ModifierFlags = []
            for name in names {
                switch name {
                case "cmd": out.insert(.command)
                case "shift": out.insert(.shift)
                case "control": out.insert(.control)
                default: break
                }
            }
            return out
        }

        hooks["setup.g.scene"] = { _ in
            await settle(150)
            return model.graph.canvas.sceneText
        }

        hooks["setup.g.state"] = { _ in
            await settle(120)
            return model.graph.canvas.stateText + " | delete button \(model.graph.toolbar.canDeleteFilter) zoom \(model.graph.toolbar.zoomPercent)"
        }

        // Clicks a shape, "-" for empty space. Then any of cmd, shift, control, double.
        hooks["setup.g.click"] = { args in
            let canvas = model.graph.canvas
            guard let name = args.first else { return "FAIL" }
            let point: CGPoint
            if name == "-" {
                point = CGPoint(x: canvas.bounds.maxX - 2, y: canvas.bounds.maxY - 2)
            } else {
                guard let found = shape(name) else { return "FAIL no shape \(name)" }
                point = found.point
            }
            let clicks = args.contains("double") ? 2 : 1
            let mods = flags(args.dropFirst())
            canvas.sendMouse(.leftMouseDown, at: point, flags: mods, clicks: clicks)
            canvas.sendMouse(.leftMouseUp, at: point, flags: mods, clicks: clicks)
            return canvas.stateText
        }

        // Drags from a shape's middle by dx, dy. "hold" stops before the release.
        hooks["setup.g.drag"] = { args in
            let canvas = model.graph.canvas
            guard args.count >= 3, let found = shape(args[0]), let dx = Double(args[1]), let dy = Double(args[2]) else { return "FAIL \(args)" }
            let mods = flags(args.dropFirst(3))
            canvas.sendMouse(.leftMouseDown, at: found.point, flags: mods)
            for i in 1...6 {
                let t = CGFloat(i) / 6
                canvas.sendMouse(.leftMouseDragged, at: CGPoint(x: found.point.x + dx * t, y: found.point.y + dy * t), flags: mods)
            }
            if !args.contains("hold") {
                canvas.sendMouse(.leftMouseUp, at: CGPoint(x: found.point.x + dx, y: found.point.y + dy), flags: mods)
            }
            return canvas.stateText
        }

        // A frame from x0, y0 to x1, y1 in canvas units. Then modifiers.
        hooks["setup.g.frame"] = { args in
            let canvas = model.graph.canvas
            let n = args.prefix(4).compactMap { Double($0) }
            guard n.count == 4 else { return "FAIL \(args)" }
            let mods = flags(args.dropFirst(4))
            canvas.sendMouse(.leftMouseDown, at: CGPoint(x: n[0], y: n[1]), flags: mods)
            canvas.sendMouse(.leftMouseDragged, at: CGPoint(x: (n[0] + n[2]) / 2, y: (n[1] + n[3]) / 2), flags: mods)
            canvas.sendMouse(.leftMouseDragged, at: CGPoint(x: n[2], y: n[3]), flags: mods)
            if !args.contains("hold") {
                canvas.sendMouse(.leftMouseUp, at: CGPoint(x: n[2], y: n[3]), flags: mods)
            }
            return canvas.stateText
        }

        // A connection: source, output port or "-" for anywhere on the shape, target or "-" for empty space, then left, right or control, then maybe hold.
        hooks["setup.g.connect"] = { args in
            let canvas = model.graph.canvas
            guard args.count >= 4, let source = shape(args[0]) else { return "FAIL \(args)" }
            let start = Int(args[1]).map { canvas.outputPoint(shape: source.index, output: $0) } ?? source.point
            let end: CGPoint
            if args[2] == "-" {
                end = CGPoint(x: canvas.bounds.maxX - 2, y: canvas.bounds.maxY - 2)
            } else {
                guard let target = shape(args[2]) else { return "FAIL no target \(args[2])" }
                end = canvas.inputPointOf(shape: target.index)
            }
            let how = args[3]
            let mods: NSEvent.ModifierFlags = how == "control" ? .control : []
            let (down, drag): (NSEvent.EventType, NSEvent.EventType) = how == "right" ? (.rightMouseDown, .rightMouseDragged) : (.leftMouseDown, how == "control" ? .rightMouseDragged : .leftMouseDragged)
            canvas.sendMouse(down, at: start, flags: mods)
            for i in 1...6 {
                let t = CGFloat(i) / 6
                canvas.sendMouse(drag, at: CGPoint(x: start.x + (end.x - start.x) * t, y: start.y + (end.y - start.y) * t), flags: mods)
            }
            let during = canvas.stateText
            if !args.contains("hold") {
                if how == "left" {
                    canvas.sendMouse(.leftMouseUp, at: end, flags: mods)
                } else {
                    _ = canvas.releaseSecondary(at: end)
                }
            }
            return during
        }

        // Clicks the middle of a line by its place in the scene. Then modifiers.
        hooks["setup.g.clickLine"] = { args in
            let canvas = model.graph.canvas
            guard let i = args.first.flatMap({ Int($0) }), i < canvas.lineCount else { return "FAIL \(args)" }
            let p = canvas.lineMidpoint(i)
            let mods = flags(args.dropFirst())
            canvas.sendMouse(.leftMouseDown, at: p, flags: mods)
            canvas.sendMouse(.leftMouseUp, at: p, flags: mods)
            return canvas.stateText
        }

        // A right click with no drag: what menu it would show, and whether the selection stayed.
        hooks["setup.g.rightClick"] = { args in
            let canvas = model.graph.canvas
            guard let found = args.first.flatMap(shape) else { return "FAIL" }
            let before = canvas.stateText
            canvas.sendMouse(.rightMouseDown, at: found.point)
            let wouldShow = canvas.releaseSecondary(at: found.point)
            let menu = canvas.contextMenu()
            return "menu \(wouldShow): \(menuText(menu)) | \(before == canvas.stateText ? "selection kept" : "CHANGED \(canvas.stateText)")"
        }

        hooks["setup.g.menuRun"] = { args in
            run(model.graph.canvas.contextMenu(), args)
        }

        hooks["setup.g.action"] = { args in
            let canvas = model.graph.canvas
            switch args.first {
            case "selectAll": canvas.selectAll(nil)
            case "copy": canvas.copy(nil)
            case "cut": canvas.cut(nil)
            case "paste": canvas.paste(nil)
            case "delete": canvas.delete(nil)
            case "deleteButton": model.graph.deleteSelected()
            case "addFilter": model.graph.addFilter()
            case "zoomIn": model.graph.zoom(in: true)
            case "zoomOut": model.graph.zoom(in: false)
            case "zoomFit": model.graph.zoomToFit()
            case "undo": canvas.undoManager?.undo()
            case "escape": canvas.cancelOperation(nil)
            case "focus": canvas.window?.makeFirstResponder(canvas)
            default: return "FAIL \(args)"
            }
            await settle(150)
            return canvas.stateText + " | zoom \(model.graph.toolbar.zoomPercent)"
        }

        // The cursor over a shape, or over one of its output ports.
        hooks["setup.g.cursor"] = { args in
            let canvas = model.graph.canvas
            guard let found = args.first.flatMap(shape) else { return "FAIL" }
            let point = args.count > 1 ? canvas.outputPoint(shape: found.index, output: Int(args[1]) ?? 0) : found.point
            canvas.sendMouse(.mouseMoved, at: point)
            return canvas.cursorName
        }

        // Shape details for checking a picture against the spec.
        hooks["setup.shapeRect"] = { names in
            guard let name = names.first, let rect = model.graph.canvas.rectOfShape(titled: name) else { return "none" }
            return "\(rect)"
        }

        UITestHarness.hooks = hooks
    }

    @MainActor
    private static func menuText(_ menu: NSMenu) -> String {
        menu.items.map { item in
            if item.isSeparatorItem { return "-" }
            var text = item.title
            if !item.isEnabled { text += " (off)" }
            switch item.state {
            case .on: text += " (on)"
            case .mixed: text += " (mixed)"
            default: break
            }
            if let sub = item.submenu {
                text += " [" + menuText(sub) + "]"
            }
            return text
        }.joined(separator: ", ")
    }

    @MainActor
    private static func run(_ menu: NSMenu, _ path: [String]) -> String {
        var current = menu
        for (i, title) in path.enumerated() {
            guard let item = current.items.first(where: { $0.title == title }) else { return "FAIL no item \(title)" }
            if i == path.count - 1 {
                guard item.isEnabled else { return "FAIL \(title) is disabled" }
                current.performActionForItem(at: current.index(of: item))
                return "ran \(title)"
            }
            guard let sub = item.submenu else { return "FAIL \(title) has no submenu" }
            current = sub
        }
        return "FAIL empty path"
    }

    /// The rows above these, top first, so that expanding them in order reveals these.
    private static func ancestors(of rows: [String]) -> [String] {
        var seen = Set<String>()
        var out: [String] = []
        for row in rows {
            let parts = row.split(separator: "/")
            for end in stride(from: 1, to: parts.count, by: 1) {
                let path = parts[..<end].joined(separator: "/")
                if seen.insert(path).inserted {
                    out.append(path)
                }
            }
        }
        return out
    }

    /// A row path for a node name, by walking down from the roots.
    @MainActor
    private static func elementRow(named name: String, model: SetupModel) -> String? {
        guard let session = model.session else { return nil }
        var stack: [String?] = [nil]
        while let parent = stack.popLast() {
            for row in session.elementRows(parent: parent) {
                if row.name == name { return row.path }
                if row.hasChildren { stack.append(row.path) }
            }
        }
        return nil
    }
}
