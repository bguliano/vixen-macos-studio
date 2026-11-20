import AppKit
import Observation
import SwiftUI
import VixenBridge
import VixenKit

/// The show window's Schedule section: the scheduler's switch, every schedule with its next run and an On switch, edited in
/// place, and every show with its steps. All edits are live on the profile's shared undo history (the one Display, Previews and
/// agents use) and the profile saves after each one. Replaces the Shows and Scheduler windows. The edits are the `show_*` and
/// `schedule_*` display commands (`studio_schedule.rs`), so an agent's changes show here at once.
struct StudioSchedulePane: View {
    /// The shared history's event revision, which moves whenever anyone (an agent, Undo) changes the display or the schedule.
    let revision: UInt64
    @Environment(AdminModel.self) private var admin
    private var model: StudioScheduleModel { .shared }

    var body: some View {
        VStack(spacing: 0) {
            content
            Divider()
            statusBar
        }
        .background(StudioScheduleWindowReader())
        .toolbar { toolbar }
        .task(id: revision) { await reload() }
        .task {
            // Next-run times and the scheduler's status move with the clock.
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(20))
                await reload()
            }
        }
        .onChange(of: admin.executionStatus.stopEnabled) { Task { await reload() } }
        .onAppear {
            model.paneVisible = true
            model.takeOverUndo()
            if UITestHarness.isActive { installTestActions() }
        }
        .onDisappear {
            model.paneVisible = false
            model.releaseUndo()
        }
    }

    private func reload() async {
        guard let core = admin.app.core else { return }
        await model.reload(core: core)
    }

    @ViewBuilder
    private var content: some View {
        if let snapshot = model.snapshot {
            ScrollView {
                VStack(alignment: .leading, spacing: StudioMetrics.spacingXL) {
                    StudioSchedulerSwitch(snapshot: snapshot)
                    schedules(snapshot)
                    shows(snapshot)
                }
                .padding(StudioMetrics.spacingL)
                .frame(maxWidth: 900, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .onDeleteCommand { model.deleteSelection(perform: perform) }
        } else {
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    @ViewBuilder
    private func schedules(_ snapshot: StudioScheduleSnapshot) -> some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            StudioScheduleHeader(title: "Schedules",
                                 detail: "When each show runs on its own. A schedule runs its show from the start time to the end time on the days ticked, from its first day to its last.")
            if snapshot.schedules.isEmpty {
                StudioScheduleEmpty(text: snapshot.shows.isEmpty
                    ? "Add a show below, then schedule it here."
                    : "No schedules yet. Add Schedule runs a show at set times and days.")
            } else {
                VStack(spacing: 0) {
                    ForEach(snapshot.schedules) { row in
                        if row.index > 0 { Divider() }
                        StudioScheduleRowView(row: row, snapshot: snapshot, perform: perform)
                    }
                }
                .studioCard(padding: 0)
            }
        }
    }

    @ViewBuilder
    private func shows(_ snapshot: StudioScheduleSnapshot) -> some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            StudioScheduleHeader(title: "Shows",
                                 detail: "A show is the list of sequences and pauses a schedule plays. Its main steps repeat in order until the schedule ends.")
            if snapshot.shows.isEmpty {
                ContentUnavailableView {
                    Label("No Shows", systemImage: "list.bullet.rectangle")
                } description: {
                    Text("Add a show, add the sequences it plays, then schedule it.")
                } actions: {
                    Button("Add Show") { perform { await model.addShow(core: $0) } }
                }
                .studioCard()
            } else {
                ForEach(snapshot.shows) { show in
                    StudioShowCard(show: show, sequences: snapshot.sequences, perform: perform)
                }
            }
        }
    }

    private var statusBar: some View {
        HStack(spacing: StudioMetrics.spacingS) {
            if let notice = model.notice {
                Label(notice, systemImage: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .lineLimit(2)
            } else {
                Text(model.status)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            Button { perform { await model.undo(core: $0) } } label: {
                Label("Undo", systemImage: "arrow.uturn.backward")
            }
            .help(model.undoHelp)
            .disabled(!model.canUndo)
            Button { perform { await model.redo(core: $0) } } label: {
                Label("Redo", systemImage: "arrow.uturn.forward")
            }
            .help(model.redoHelp)
            .disabled(!model.canRedo)
        }
        .labelStyle(.titleAndIcon)
        .font(.callout)
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, StudioMetrics.spacingS)
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            Button { model.duplicateSelection(perform: perform) } label: {
                Label("Duplicate", systemImage: "plus.square.on.square")
            }
            .help(model.selection == nil ? "Select a show or schedule to duplicate" : "Duplicate the selected \(model.selectionNoun)")
            .disabled(model.selection == nil || model.working)
            Button { model.deleteSelection(perform: perform) } label: {
                Label("Delete", systemImage: "trash")
            }
            .help(model.selection == nil ? "Select a show or schedule to delete" : "Delete the selected \(model.selectionNoun). Undo brings it back.")
            .disabled(model.selection == nil || model.working)
            Button { perform { await model.addSchedule(core: $0) } } label: {
                Label("Add Schedule", systemImage: "calendar.badge.plus")
            }
            .help(model.snapshot?.shows.isEmpty == false ? "Schedule a show, starting today" : "Add a show first, then schedule it")
            .disabled(model.working || model.snapshot?.shows.isEmpty != false)
            Button { perform { await model.addShow(core: $0) } } label: {
                Label("Add Show", systemImage: "plus")
            }
            .help("Add an empty show")
            .disabled(model.working || model.snapshot == nil)
        }
    }

    private func perform(_ action: @escaping (VixenCore) async -> Void) {
        guard let core = admin.app.core else { return }
        Task { await action(core) }
    }

    /// Rows and menus aren't reachable by a script's click: these pick a show or schedule by name or position, rename,
    /// run the step menus' commands as their items do, and report the pane's state.
    private func installTestActions() {
        UITestHarness.customActions["studioScheduleSelectShow"] = { step in
            guard let show = model.snapshot?.shows.first(where: { $0.name == step.name }) else { return (false, "no show \(step.name ?? "")") }
            model.selection = .show(show.id)
            return (true, nil)
        }
        UITestHarness.customActions["studioScheduleSelectSchedule"] = { step in
            guard let index = step.name.flatMap(Int.init), model.snapshot?.schedules.indices.contains(index) == true else { return (false, "no schedule \(step.name ?? "")") }
            model.selection = .schedule(index)
            return (true, nil)
        }
        UITestHarness.customActions["studioScheduleRenameShow"] = { _ in
            guard case .show(let id) = model.selection else { return (false, "no show selected") }
            model.renaming = id
            return (true, nil)
        }
        // `name` is "<section>:<sequence file>" or "<section>:pause"; added to the selected show, as Add ▸ does.
        UITestHarness.customActions["studioScheduleAddStep"] = { step in
            guard case .show(let id) = model.selection, let parts = step.name?.split(separator: ":", maxSplits: 1), parts.count == 2 else { return (false, "select a show; name section:sequence") }
            let section = String(parts[0]), what = String(parts[1])
            perform { core in
                if what == "pause" {
                    await model.addPause(show: id, section: section, core: core)
                } else {
                    await model.addSequence(show: id, section: section, sequence: what, core: core)
                }
            }
            return (true, nil)
        }
        // `name` is "<section>:<from>:<to>" in the selected show, as Move Up/Down do.
        UITestHarness.customActions["studioScheduleMoveStep"] = { step in
            guard case .show(let id) = model.selection, let parts = step.name?.split(separator: ":"), parts.count == 3,
                  let from = Int(parts[1]), let to = Int(parts[2]) else { return (false, "select a show; name section:from:to") }
            perform { await model.moveStep(show: id, section: String(parts[0]), from: from, to: to, core: $0) }
            return (true, nil)
        }
        // `name` is "<field>=<value>" for the selected schedule: startTime, endTime, startDate, endDate, show (a show name) or
        // days (seven 0/1, Sunday first), as its editor's controls set them.
        UITestHarness.customActions["studioScheduleSetField"] = { step in
            guard case .schedule(let index) = model.selection, let parts = step.name?.split(separator: "=", maxSplits: 1), parts.count == 2 else { return (false, "select a schedule; name field=value") }
            let field = String(parts[0]), value = String(parts[1])
            var arguments: [String: Any]
            switch field {
            case "show":
                guard let show = model.snapshot?.shows.first(where: { $0.name == value }) else { return (false, "no show \(value)") }
                arguments = ["showId": show.id]
            case "days":
                arguments = ["days": value.map { $0 == "1" }]
            default:
                arguments = [field: value]
            }
            perform { await model.updateSchedule(index: index, arguments, core: $0) }
            return (true, nil)
        }
        UITestHarness.customActions["studioScheduleMenuUndo"] = { _ in
            guard let manager = model.window?.undoManager, manager.canUndo else { return (false, "Edit ▸ Undo is off") }
            let title = manager.undoMenuItemTitle
            manager.undo()
            return (true, title)
        }
        UITestHarness.customActions["studioScheduleMenuRedo"] = { _ in
            guard let manager = model.window?.undoManager, manager.canRedo else { return (false, "Edit ▸ Redo is off") }
            let title = manager.redoMenuItemTitle
            manager.redo()
            return (true, title)
        }
        UITestHarness.customActions["studioScheduleState"] = { _ in
            guard let snapshot = model.snapshot else { return (false, "not loaded") }
            let shows = snapshot.shows.map { show in
                "\(show.name)[\(show.schedules)]: " + StudioShowSection.all.compactMap { section in
                    let steps = show.steps(section.id)
                    return steps.isEmpty ? nil : "\(section.id)=" + steps.map(\.title).joined(separator: ",")
                }.joined(separator: " ")
            }.joined(separator: "; ")
            let schedules = snapshot.schedules.map { row in
                "\(row.showName ?? "-") \(row.startDate ?? "-")..\(row.endDate ?? "-") \(row.startTime)-\(row.endTime) \(StudioScheduleText.days(row.days)) \(row.enabled ? "on" : "off") \(row.state)"
            }.joined(separator: "; ")
            return (true, "shows: \(shows) | schedules: \(schedules) | selected=\(model.selectionDescription) renaming=\(model.renaming == nil ? "no" : "yes") undo=\(model.undoLabel ?? "-") redo=\(model.redoLabel ?? "-") menu=\(model.window?.undoManager?.undoMenuItemTitle ?? "-")/\(model.window?.undoManager?.redoMenuItemTitle ?? "-") schedulerOn=\(snapshot.schedulerOn) status=\(model.status) notice=\(model.notice ?? "-")")
        }
    }
}

// MARK: Pieces

private struct StudioScheduleHeader: View {
    let title: String
    let detail: String

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.title2.weight(.semibold))
            Text(detail)
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

private struct StudioScheduleEmpty: View {
    let text: String

    var body: some View {
        Text(text)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .studioCard()
    }
}

/// Scheduler ▸ Enabled, in place: schedules run only while it is on. A local switch; agents can't turn it.
private struct StudioSchedulerSwitch: View {
    let snapshot: StudioScheduleSnapshot
    @Environment(AdminModel.self) private var admin

    var body: some View {
        let toggle = admin.app.schedulerEnabledToggle
        let on = toggle?.isOn() ?? snapshot.schedulerOn
        HStack(alignment: .center, spacing: StudioMetrics.spacingM) {
            Image(systemName: on ? "calendar.badge.clock" : "calendar")
                .font(.title2)
                .foregroundStyle(on ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                .frame(width: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(on ? "Schedules are running" : "Schedules are paused")
                    .font(.headline)
                Text(detail(on: on))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: StudioMetrics.spacingM)
            Toggle("Run Schedules", isOn: Binding(get: { on }, set: { new in
                toggle?.setOn(new)
                Task {
                    if let core = admin.app.core { await StudioScheduleModel.shared.reload(core: core) }
                }
            }))
            .labelsHidden()
            .toggleStyle(.switch)
            .disabled(toggle == nil)
            .help(on ? "Pause every schedule. A show playing now stops." : "Let schedules that are on run their shows. Same as Scheduler ▸ Enabled.")
        }
        .studioCard()
    }

    private func detail(on: Bool) -> String {
        if on, let status = snapshot.status, !status.isEmpty { return status }
        if on { return "Schedules that are on play their shows at their times." }
        let next = snapshot.schedules.filter { $0.enabled && $0.nextRun != nil }.count
        return next > 0
            ? "Nothing plays until you turn this on, even schedules that are on. Same as Scheduler ▸ Enabled."
            : "Schedules play their shows only while this is on. Same as Scheduler ▸ Enabled."
    }
}

/// One schedule: On switch, show and times at a glance, and its next run. Selected, it opens its editor in place.
private struct StudioScheduleRowView: View {
    let row: StudioScheduleRow
    let snapshot: StudioScheduleSnapshot
    let perform: (@escaping (VixenCore) async -> Void) -> Void
    private var model: StudioScheduleModel { .shared }
    private var selected: Bool { model.selection == .schedule(row.index) }

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingM) {
            HStack(alignment: .center, spacing: StudioMetrics.spacingM) {
                Toggle("Turn on schedule for \(row.showName ?? "no show")", isOn: Binding(get: { row.enabled }, set: { on in
                    perform { await model.setScheduleEnabled(index: row.index, on: on, core: $0) }
                }))
                .labelsHidden()
                .toggleStyle(.switch)
                .controlSize(.small)
                .help(row.enabled ? "On: runs while schedules are running. Turn off to skip it." : "Off: never runs. Turn on to run it at its times.")
                VStack(alignment: .leading, spacing: 2) {
                    Text(row.showName ?? "No show chosen")
                        .font(.headline)
                        .foregroundStyle(row.showName == nil ? .secondary : .primary)
                        .lineLimit(1)
                    Text(StudioScheduleText.summary(row))
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer(minLength: StudioMetrics.spacingS)
                StudioScheduleStateBadge(row: row, now: snapshot.now)
                Image(systemName: "chevron.right")
                    .rotationEffect(.degrees(selected ? 90 : 0))
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
            if selected {
                StudioScheduleEditor(row: row, shows: snapshot.shows, perform: perform)
            }
        }
        .padding(StudioMetrics.spacingM)
        .background(selected ? AnyShapeStyle(.selection.opacity(0.18)) : AnyShapeStyle(.clear))
        .contentShape(Rectangle())
        .onTapGesture { model.selection = selected ? nil : .schedule(row.index) }
        .contextMenu {
            Button(row.enabled ? "Turn Off" : "Turn On") { perform { await model.setScheduleEnabled(index: row.index, on: !row.enabled, core: $0) } }
            Button("Duplicate") { perform { await model.duplicateSchedule(index: row.index, core: $0) } }
            Divider()
            Button("Delete", role: .destructive) { perform { await model.deleteSchedule(index: row.index, core: $0) } }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Schedule for \(row.showName ?? "no show")")
    }
}

private struct StudioScheduleStateBadge: View {
    let row: StudioScheduleRow
    let now: String

    var body: some View {
        switch row.state {
        case "playing": StudioStatusBadge("Playing now", systemImage: "play.fill", tone: .active)
        case "due": StudioStatusBadge("Due now", tone: .active)
        case "upcoming": StudioStatusBadge(row.nextRun.map { StudioScheduleText.next($0, now: now) } ?? "Upcoming", tone: .neutral)
        case "ended": StudioStatusBadge("Ended", tone: .warning)
        case "never": StudioStatusBadge("No days ticked", tone: .warning)
                .help("None of its days fall between its first and last day")
        default: StudioStatusBadge("Off", tone: .neutral)
        }
    }
}

/// A schedule's fields, each applied as it changes (one undo step each).
private struct StudioScheduleEditor: View {
    let row: StudioScheduleRow
    let shows: [StudioShowRow]
    let perform: (@escaping (VixenCore) async -> Void) -> Void
    private var model: StudioScheduleModel { .shared }

    var body: some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: StudioMetrics.spacingM, verticalSpacing: StudioMetrics.spacingS) {
            GridRow {
                Text("Show").foregroundStyle(.secondary).gridColumnAlignment(.trailing)
                Picker("Show", selection: Binding(get: { row.showId ?? "" }, set: { id in
                    guard !id.isEmpty, id != row.showId else { return }
                    update(["showId": id])
                })) {
                    if row.showId == nil { Text("Choose a show").tag("") }
                    ForEach(shows) { Text($0.name).tag($0.id) }
                }
                .labelsHidden()
                .fixedSize()
            }
            GridRow {
                Text("Dates").foregroundStyle(.secondary)
                HStack(spacing: StudioMetrics.spacingS) {
                    DatePicker("First day", selection: dateBinding(row.startDate, key: "startDate"), displayedComponents: .date)
                        .labelsHidden()
                    Text("to")
                    DatePicker("Last day", selection: dateBinding(row.endDate, key: "endDate"), displayedComponents: .date)
                        .labelsHidden()
                    Text("both days included").font(.caption).foregroundStyle(.secondary)
                }
            }
            GridRow {
                Text("Time").foregroundStyle(.secondary)
                HStack(spacing: StudioMetrics.spacingS) {
                    DatePicker("Start time", selection: timeBinding(row.startTime, key: "startTime"), displayedComponents: .hourAndMinute)
                        .labelsHidden()
                    Text("to")
                    DatePicker("End time", selection: timeBinding(row.endTime, key: "endTime"), displayedComponents: .hourAndMinute)
                        .labelsHidden()
                    Text(StudioScheduleText.length(row.startTime, row.endTime))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            GridRow {
                Text("Days").foregroundStyle(.secondary)
                HStack(spacing: StudioMetrics.spacingXS) {
                    ForEach(0..<7, id: \.self) { day in
                        let on = row.days[day]
                        Button {
                            var days = row.days
                            days[day] = !on
                            update(["days": days])
                        } label: {
                            Text(StudioScheduleText.dayLetters[day])
                                .font(.callout.weight(.semibold))
                                .frame(width: 26, height: 26)
                                .foregroundStyle(on ? AnyShapeStyle(.white) : AnyShapeStyle(.secondary))
                                .background(Circle().fill(on ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary)))
                                .contentShape(Circle())
                        }
                        .buttonStyle(.plain)
                        .help("\(Calendar.current.weekdaySymbols[day]): \(on ? "runs" : "doesn't run"). Click to change.")
                        .accessibilityLabel(Calendar.current.weekdaySymbols[day])
                        .accessibilityAddTraits(on ? .isSelected : [])
                    }
                    Menu("Presets") {
                        Button("Every Day") { update(["days": Array(repeating: true, count: 7)]) }
                        Button("Weekdays") { update(["days": [false, true, true, true, true, true, false]]) }
                        Button("Weekends") { update(["days": [true, false, false, false, false, false, true]]) }
                        Button("Friday and Saturday") { update(["days": [false, false, false, false, false, true, true]]) }
                    }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                }
            }
        }
        .padding(.leading, 44)
        .disabled(model.working)
    }

    private func update(_ arguments: [String: Any]) {
        perform { await model.updateSchedule(index: row.index, arguments, core: $0) }
    }

    private func dateBinding(_ text: String?, key: String) -> Binding<Date> {
        Binding(get: { StudioScheduleText.date(text) ?? Date() }, set: { date in
            let value = StudioScheduleText.dateText(date)
            if value != text { update([key: value]) }
        })
    }

    private func timeBinding(_ text: String, key: String) -> Binding<Date> {
        Binding(get: { StudioScheduleText.time(text) }, set: { date in
            let value = StudioScheduleText.timeText(date)
            if value != text { update([key: value]) }
        })
    }
}

/// A show's sections, in the order a run takes them, in plain words.
struct StudioShowSection: Identifiable {
    let id: String
    let title: String
    let detail: String

    static let all = [
        StudioShowSection(id: "startup", title: "Before the show", detail: "Plays once when the schedule starts"),
        StudioShowSection(id: "sequential", title: "Show", detail: "Plays in order, again and again until the schedule ends"),
        StudioShowSection(id: "background", title: "Alongside", detail: "Plays in the background the whole time"),
        StudioShowSection(id: "shutdown", title: "After the show", detail: "Plays once when the schedule ends"),
    ]
}

/// One show: its name (double-click to rename), how many schedules run it, and its steps by section.
private struct StudioShowCard: View {
    let show: StudioShowRow
    let sequences: [String]
    let perform: (@escaping (VixenCore) async -> Void) -> Void
    private var model: StudioScheduleModel { .shared }
    @State private var draft = ""
    @FocusState private var nameFocused: Bool
    private var selected: Bool { model.selection == .show(show.id) }

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingM) {
            HStack(alignment: .firstTextBaseline, spacing: StudioMetrics.spacingS) {
                name
                Text(show.schedules == 0 ? "Not scheduled" : show.schedules == 1 ? "1 schedule" : "\(show.schedules) schedules")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Spacer()
                addMenu
            }
            ForEach(StudioShowSection.all) { section in
                let steps = show.steps(section.id)
                if !steps.isEmpty || section.id == "sequential" {
                    sectionView(section, steps: steps)
                }
            }
        }
        .padding(StudioMetrics.spacingM)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: StudioMetrics.cornerRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: StudioMetrics.cornerRadius, style: .continuous)
                .strokeBorder(selected ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator.opacity(0.6)), lineWidth: selected ? 2 : 0.5)
        )
        .contentShape(Rectangle())
        .onTapGesture { model.selection = .show(show.id) }
        .contextMenu {
            Button("Rename") { startRename() }
            Button("Duplicate") { perform { await model.duplicateShow(id: show.id, core: $0) } }
            Divider()
            Button("Delete", role: .destructive) { perform { await model.deleteShow(id: show.id, core: $0) } }
        }
        .task(id: model.renaming == show.id) {
            guard model.renaming == show.id else { return }
            draft = show.name
            nameFocused = true
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Show \(show.name)")
    }

    @ViewBuilder
    private var name: some View {
        if model.renaming == show.id {
            TextField("Name", text: $draft)
                .textFieldStyle(.roundedBorder)
                .font(.headline)
                .frame(maxWidth: 320)
                .focused($nameFocused)
                .onSubmit(commitRename)
                .onExitCommand { model.renaming = nil }
                .onChange(of: nameFocused) { _, focused in
                    if !focused, model.renaming == show.id { commitRename() }
                }
        } else {
            Text(show.name.isEmpty ? "Untitled Show" : show.name)
                .font(.headline)
                .lineLimit(1)
                .truncationMode(.middle)
                .onTapGesture(count: 2) { startRename() }
                .help("Double-click to rename")
        }
    }

    private var addMenu: some View {
        Menu {
            ForEach(StudioShowSection.all) { section in
                Section(section.title) {
                    sequenceMenu(title: "Sequence") { file in
                        perform { await model.addSequence(show: show.id, section: section.id, sequence: file, core: $0) }
                    }
                    Button("Pause") { perform { await model.addPause(show: show.id, section: section.id, core: $0) } }
                }
            }
        } label: {
            Label("Add", systemImage: "plus")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("Add a sequence or a pause to this show")
        .disabled(model.working)
    }

    @ViewBuilder
    private func sequenceMenu(title: String, choose: @escaping (String) -> Void) -> some View {
        Menu(title) {
            if sequences.isEmpty {
                Text("No sequences in this profile")
            }
            ForEach(sequences, id: \.self) { file in
                Button(StudioScheduleText.sequenceTitle(file)) { choose(file) }
            }
        }
    }

    private func sectionView(_ section: StudioShowSection, steps: [StudioShowStep]) -> some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
            HStack(alignment: .firstTextBaseline) {
                Text(section.title).font(.subheadline.weight(.semibold))
                Text(section.detail).font(.caption).foregroundStyle(.secondary)
            }
            if steps.isEmpty {
                Text("Nothing yet. Add ▸ \(section.title) ▸ Sequence picks what it plays.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .padding(.vertical, StudioMetrics.spacingXS)
            }
            ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                stepRow(section: section.id, index: index, count: steps.count, step: step)
            }
        }
    }

    private func stepRow(section: String, index: Int, count: Int, step: StudioShowStep) -> some View {
        HStack(spacing: StudioMetrics.spacingS) {
            Text("\(index + 1)")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 18, alignment: .trailing)
            Image(systemName: step.symbol)
                .foregroundStyle(step.missing == true || step.action == "launch" ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
                .frame(width: 18)
            if step.action == "pause" {
                Stepper(value: Binding(get: { step.pauseSeconds ?? 1 }, set: { seconds in
                    perform { await model.setPause(show: show.id, section: section, index: index, seconds: seconds, core: $0) }
                }), in: 1...900) {
                    Text("Pause for \(StudioScheduleText.seconds(step.pauseSeconds ?? 1))")
                }
                .fixedSize()
            } else {
                Text(step.title)
                    .lineLimit(1)
                    .truncationMode(.middle)
                if step.missing == true {
                    Text("File missing").font(.caption).foregroundStyle(.orange)
                } else if step.action == "launch" {
                    Text("Runs a program; edit it in Windows Vixen").font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: StudioMetrics.spacingS)
            Menu {
                if step.action != "launch" {
                    sequenceMenu(title: step.action == "sequence" ? "Change Sequence" : "Play a Sequence Instead") { file in
                        perform { await model.setSequence(show: show.id, section: section, index: index, sequence: file, core: $0) }
                    }
                    if step.action != "pause" {
                        Button("Pause Instead") { perform { await model.setPause(show: show.id, section: section, index: index, seconds: 5, core: $0) } }
                    }
                    Divider()
                }
                Button("Move Up") { perform { await model.moveStep(show: show.id, section: section, from: index, to: index - 1, core: $0) } }
                    .disabled(index == 0)
                Button("Move Down") { perform { await model.moveStep(show: show.id, section: section, from: index, to: index + 1, core: $0) } }
                    .disabled(index == count - 1)
                Divider()
                Button("Remove", role: .destructive) { perform { await model.removeStep(show: show.id, section: section, index: index, core: $0) } }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("Change, move or remove this step")
            .accessibilityLabel("Step \(index + 1) options")
        }
        .padding(.vertical, 2)
        .disabled(model.working)
    }

    private func startRename() {
        model.selection = .show(show.id)
        model.renaming = show.id
    }

    private func commitRename() {
        let name = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        model.renaming = nil
        guard !name.isEmpty, name != show.name else { return }
        perform { await model.renameShow(id: show.id, name: name, core: $0) }
    }
}

// MARK: Data

struct StudioScheduleSnapshot: Decodable, Equatable, Sendable {
    let revision: UInt64
    let liveOutput: Bool
    let schedulerOn: Bool
    let status: String?
    let now: String
    let shows: [StudioShowRow]
    let schedules: [StudioScheduleRow]
    let sequences: [String]
}

struct StudioShowRow: Decodable, Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    let schedules: Int
    let sections: [String: [StudioShowStep]]

    func steps(_ section: String) -> [StudioShowStep] { sections[section] ?? [] }
}

struct StudioShowStep: Decodable, Equatable, Sendable {
    let action: String
    let name: String
    let sequence: String?
    let missing: Bool?
    let pauseSeconds: Int?
    let program: String?

    var title: String {
        switch action {
        case "sequence": StudioScheduleText.sequenceTitle(sequence ?? name)
        case "pause": "Pause for \(StudioScheduleText.seconds(pauseSeconds ?? 1))"
        case "launch": "Run \((program ?? "").isEmpty ? "a program" : (program! as NSString).lastPathComponent)"
        default: name.isEmpty ? "Unknown step" : name
        }
    }

    var symbol: String {
        switch action {
        case "sequence": missing == true ? "exclamationmark.triangle" : "music.note"
        case "pause": "pause.circle"
        case "launch": "terminal"
        default: "questionmark.circle"
        }
    }
}

struct StudioScheduleRow: Decodable, Identifiable, Equatable, Sendable {
    let index: Int
    let showId: String?
    let showName: String?
    let enabled: Bool
    let startDate: String?
    let endDate: String?
    let startTime: String
    let endTime: String
    let days: [Bool]
    let state: String
    let nextRun: String?
    var id: Int { index }
}

private struct StudioScheduleHistory: Decodable, Sendable {
    let undo: [String]
    let redo: [String]
    let undoPreviewOnly: Bool
    let redoPreviewOnly: Bool
}

/// How the pane words dates, times and days. Stored values are local wall-clock text (`2026-11-27`, `17:30`).
enum StudioScheduleText {
    static let dayLetters = ["S", "M", "T", "W", "T", "F", "S"]

    static func days(_ days: [Bool]) -> String {
        if days.allSatisfy({ $0 }) { return "Every day" }
        if !days.contains(true) { return "No days" }
        if days == [false, true, true, true, true, true, false] { return "Weekdays" }
        if days == [true, false, false, false, false, false, true] { return "Weekends" }
        let names = Calendar.current.shortWeekdaySymbols
        // Monday first, Sunday last, as people list a week's evenings.
        return [1, 2, 3, 4, 5, 6, 0].filter { days[$0] }.map { names[$0] }.joined(separator: ", ")
    }

    static func summary(_ row: StudioScheduleRow) -> String {
        let dates: String
        switch (date(row.startDate), date(row.endDate)) {
        case let (start?, end?):
            let sameYear = Calendar.current.component(.year, from: start) == Calendar.current.component(.year, from: end)
            let style = Date.FormatStyle().month(.abbreviated).day()
            dates = sameYear && start == end
                ? start.formatted(style.year())
                : "\(start.formatted(sameYear ? style : style.year())) – \(end.formatted(style.year()))"
        default:
            dates = "No dates set"
        }
        return "\(days(row.days)), \(clock(row.startTime)) – \(clock(row.endTime)) · \(dates)"
    }

    static func next(_ iso: String, now: String) -> String {
        guard let date = localDate(iso) else { return "Upcoming" }
        let calendar = Calendar.current
        let time = date.formatted(date: .omitted, time: .shortened)
        if let today = localDate(now), calendar.isDate(date, inSameDayAs: today) {
            return calendar.component(.hour, from: date) >= 17 ? "Tonight at \(time)" : "Today at \(time)"
        }
        if let today = localDate(now), let tomorrow = calendar.date(byAdding: .day, value: 1, to: today), calendar.isDate(date, inSameDayAs: tomorrow) {
            return "Tomorrow at \(time)"
        }
        return "\(date.formatted(.dateTime.weekday(.abbreviated).month(.abbreviated).day())) at \(time)"
    }

    static func length(_ start: String, _ end: String) -> String {
        let minutes = { (text: String) -> Int in
            let parts = text.split(separator: ":").compactMap { Int($0) }
            return parts.count >= 2 ? parts[0] * 60 + parts[1] : 0
        }
        var length = minutes(end) - minutes(start)
        let overnight = length < 0
        if overnight { length += 24 * 60 }
        let text = length % 60 == 0 ? "\(length / 60) h" : "\(length / 60) h \(length % 60) min"
        return overnight ? "\(text), past midnight" : text
    }

    static func seconds(_ seconds: Int) -> String {
        seconds < 60 || seconds % 60 != 0 ? "\(seconds) s" : "\(seconds / 60) min"
    }

    static func sequenceTitle(_ file: String) -> String {
        let name = (file as NSString).lastPathComponent
        return name.lowercased().hasSuffix(".tim") ? String(name.dropLast(4)) : name
    }

    static func clock(_ text: String) -> String {
        time(text).formatted(date: .omitted, time: .shortened)
    }

    static func date(_ text: String?) -> Date? {
        guard let parts = text?.split(separator: "-").compactMap({ Int($0) }), parts.count == 3 else { return nil }
        return Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
    }

    static func dateText(_ date: Date) -> String {
        let c = Calendar.current.dateComponents([.year, .month, .day], from: date)
        return String(format: "%04d-%02d-%02d", c.year ?? 2000, c.month ?? 1, c.day ?? 1)
    }

    static func time(_ text: String) -> Date {
        let parts = text.split(separator: ":").compactMap { Int($0) }
        return Calendar.current.date(bySettingHour: parts.first ?? 0, minute: parts.count > 1 ? parts[1] : 0, second: 0, of: Date()) ?? Date()
    }

    static func timeText(_ date: Date) -> String {
        let c = Calendar.current.dateComponents([.hour, .minute], from: date)
        return String(format: "%02d:%02d", c.hour ?? 0, c.minute ?? 0)
    }

    /// `2026-11-27T17:30:00`, local.
    static func localDate(_ iso: String) -> Date? {
        let parts = iso.split(whereSeparator: { "-T:".contains($0) }).compactMap { Int($0) }
        guard parts.count >= 5 else { return nil }
        return Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2], hour: parts[3], minute: parts[4]))
    }
}

// MARK: Model

/// The pane's state and work. One per app, so Edit ▸ Undo reaches it as the Previews pane's does.
@MainActor @Observable
final class StudioScheduleModel {
    static let shared = StudioScheduleModel()

    enum Selection: Hashable {
        case show(String)
        case schedule(Int)
    }

    private(set) var snapshot: StudioScheduleSnapshot?
    var selection: Selection?
    var renaming: String?
    private(set) var notice: String?
    private(set) var status = "Edits apply as you make them"
    private(set) var undoLabel: String?
    private(set) var redoLabel: String?
    private(set) var working = false
    private var undoNoGraph = false
    private var redoNoGraph = false
    @ObservationIgnored var paneVisible = false
    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored private var history = ""
    @ObservationIgnored private let undoTarget = UndoTarget()
    @ObservationIgnored private var saveTask: Task<Void, Never>?
    @ObservationIgnored private var saveAgain = false

    private final class UndoTarget: NSObject {}

    private var liveOutput: Bool { snapshot?.liveOutput ?? false }
    /// A display step (from the Display pane) undoes only while Live Output is off; a schedule or preview step always.
    var canUndo: Bool { undoLabel != nil && !working && (undoNoGraph || !liveOutput) }
    var canRedo: Bool { redoLabel != nil && !working && (redoNoGraph || !liveOutput) }
    var undoHelp: String { help(undoLabel, noGraph: undoNoGraph, verb: "Undo") }
    var redoHelp: String { help(redoLabel, noGraph: redoNoGraph, verb: "Redo") }

    var selectionNoun: String {
        switch selection {
        case .show: "show"
        case .schedule: "schedule"
        case nil: "item"
        }
    }

    var selectionDescription: String {
        switch selection {
        case .show(let id): "show:\(snapshot?.shows.first { $0.id == id }?.name ?? "?")"
        case .schedule(let index): "schedule:\(index)"
        case nil: "-"
        }
    }

    private func help(_ label: String?, noGraph: Bool, verb: String) -> String {
        guard let label else { return "Nothing to \(verb.lowercased())" }
        if !noGraph && liveOutput { return "Turn off Live Output to \(verb.lowercased()) \(StudioPreviewsModel.pretty(label)), a display change" }
        return "\(verb) \(StudioPreviewsModel.pretty(label))"
    }

    // MARK: Loading

    func reload(core: VixenCore) async {
        let (text, historyText) = await Task.detached { (core.studioScheduleList(), core.studioDisplayHistory()) }.value
        guard let next = try? JSONDecoder().decode(StudioScheduleSnapshot.self, from: Data(text.utf8)) else { return }
        if next != snapshot { snapshot = next }
        switch selection {
        case .show(let id) where !next.shows.contains(where: { $0.id == id }): selection = nil
        case .schedule(let index) where !next.schedules.indices.contains(index): selection = nil
        default: break
        }
        applyHistory(historyText, mode: .rebuild)
    }

    // MARK: Edits

    /// One display command, the same the MCP tool runs. Returns its result, or nil when it was refused.
    @discardableResult
    private func run(_ command: String, _ arguments: [String: Any], core: VixenCore, history mode: HistoryMode = .register) async -> [String: Any]? {
        guard !working else { return nil }
        var arguments = arguments
        // Schedules are addressed by position, which holds only for the list this pane last read.
        if command.hasPrefix("schedule_"), command != "schedule_create", let revision = snapshot?.revision {
            arguments["expectedRevision"] = revision
        }
        guard let data = try? JSONSerialization.data(withJSONObject: arguments), let json = String(data: data, encoding: .utf8) else { return nil }
        working = true
        let outcome: Result<String, StudioScheduleFailure> = await Task.detached {
            do { return .success(try core.studioDisplayCommand(command: command, argumentsJson: json)) } catch { return .failure(StudioScheduleFailure(error)) }
        }.value
        working = false
        switch outcome {
        case .success(let reply):
            notice = nil
            let object = (try? JSONSerialization.jsonObject(with: Data(reply.utf8))) as? [String: Any] ?? [:]
            if object["changed"] as? Bool == true {
                await refreshHistory(core: core, mode: mode)
                scheduleSave(core: core)
            }
            await reload(core: core)
            return object["result"] as? [String: Any] ?? [:]
        case .failure(let failure):
            notice = failure.text == "Display changed; refresh before retrying" ? "Someone else just changed the schedule. Try again." : failure.text
            await reload(core: core)
            return nil
        }
    }

    func addShow(core: VixenCore) async {
        guard let result = await run("show_create", [:], core: core), let id = result["showId"] as? String else { return }
        selection = .show(id)
        renaming = id
        status = "Added “\(result["name"] as? String ?? "New Show")”. Name it, then use Add to pick its sequences."
    }

    func renameShow(id: String, name: String, core: VixenCore) async {
        await run("show_rename", ["showId": id, "name": name], core: core)
    }

    func duplicateShow(id: String, core: VixenCore) async {
        guard let result = await run("show_duplicate", ["showId": id], core: core), let copy = result["showId"] as? String else { return }
        selection = .show(copy)
    }

    func deleteShow(id: String, core: VixenCore) async {
        guard let show = snapshot?.shows.first(where: { $0.id == id }) else { return }
        // Deleting a show takes its schedules with it in the same step; Undo brings both back.
        guard let result = await run("show_delete", ["showId": id, "confirm": true, "deleteSchedules": true], core: core) else { return }
        selection = nil
        let schedules = result["deletedSchedules"] as? Int ?? 0
        status = schedules == 0
            ? "Deleted “\(show.name)”. ⌘Z brings it back."
            : "Deleted “\(show.name)” and its \(schedules == 1 ? "schedule" : "\(schedules) schedules"). ⌘Z brings them back."
    }

    func addSequence(show: String, section: String, sequence: String, core: VixenCore) async {
        await run("show_item_add", ["showId": show, "section": section, "sequence": sequence], core: core)
    }

    func addPause(show: String, section: String, core: VixenCore) async {
        await run("show_item_add", ["showId": show, "section": section, "pauseSeconds": 5], core: core)
    }

    func setSequence(show: String, section: String, index: Int, sequence: String, core: VixenCore) async {
        await run("show_item_update", ["showId": show, "section": section, "index": index, "sequence": sequence], core: core)
    }

    func setPause(show: String, section: String, index: Int, seconds: Int, core: VixenCore) async {
        await run("show_item_update", ["showId": show, "section": section, "index": index, "pauseSeconds": seconds], core: core)
    }

    func moveStep(show: String, section: String, from: Int, to: Int, core: VixenCore) async {
        guard to >= 0 else { return }
        await run("show_item_move", ["showId": show, "section": section, "from": from, "to": to], core: core)
    }

    func removeStep(show: String, section: String, index: Int, core: VixenCore) async {
        await run("show_item_delete", ["showId": show, "section": section, "index": index], core: core)
    }

    /// A new schedule runs the selected show (or the first), every day from today until Twelfth Night (or for a month once
    /// that has passed), 5:30 to 10 PM, and is on.
    func addSchedule(core: VixenCore) async {
        guard let shows = snapshot?.shows, let first = shows.first else { return }
        var show = first.id
        if case .show(let id) = selection { show = id }
        let calendar = Calendar.current
        let today = Date()
        let year = calendar.component(.year, from: today)
        let month = calendar.component(.month, from: today)
        let twelfth = calendar.date(from: DateComponents(year: month >= 10 ? year + 1 : year, month: 1, day: 6)) ?? today
        let end = twelfth > today ? twelfth : calendar.date(byAdding: .month, value: 1, to: today) ?? today
        let arguments: [String: Any] = ["showId": show, "startDate": StudioScheduleText.dateText(today), "endDate": StudioScheduleText.dateText(end),
                                        "startTime": "17:30", "endTime": "22:00", "enabled": true]
        guard let result = await run("schedule_create", arguments, core: core), let index = result["index"] as? Int else { return }
        selection = .schedule(index)
        status = "Added a schedule. Set its dates, times and days below."
    }

    func updateSchedule(index: Int, _ arguments: [String: Any], core: VixenCore) async {
        var arguments = arguments
        arguments["index"] = index
        await run("schedule_update", arguments, core: core)
    }

    func setScheduleEnabled(index: Int, on: Bool, core: VixenCore) async {
        await run("schedule_enable", ["index": index, "enabled": on], core: core)
    }

    func duplicateSchedule(index: Int, core: VixenCore) async {
        guard let result = await run("schedule_duplicate", ["index": index], core: core), let copy = result["index"] as? Int else { return }
        selection = .schedule(copy)
        status = "Duplicated the schedule; the copy is off until you turn it on."
    }

    func deleteSchedule(index: Int, core: VixenCore) async {
        guard await run("schedule_delete", ["index": index, "confirm": true], core: core) != nil else { return }
        selection = nil
        status = "Deleted the schedule. ⌘Z brings it back."
    }

    func duplicateSelection(perform: (@escaping (VixenCore) async -> Void) -> Void) {
        switch selection {
        case .show(let id): perform { await self.duplicateShow(id: id, core: $0) }
        case .schedule(let index): perform { await self.duplicateSchedule(index: index, core: $0) }
        case nil: break
        }
    }

    func deleteSelection(perform: (@escaping (VixenCore) async -> Void) -> Void) {
        switch selection {
        case .show(let id): perform { await self.deleteShow(id: id, core: $0) }
        case .schedule(let index): perform { await self.deleteSchedule(index: index, core: $0) }
        case nil: break
        }
    }

    // MARK: Undo

    func undo(core: VixenCore) async {
        if let manager = window?.undoManager, manager.canUndo, manager.undoActionName == undoLabel.map(StudioPreviewsModel.pretty) {
            manager.undo()
        } else {
            await step(undo: true, core: core, fromMenu: false)
        }
    }

    func redo(core: VixenCore) async {
        if let manager = window?.undoManager, manager.canRedo, manager.redoActionName == redoLabel.map(StudioPreviewsModel.pretty) {
            manager.redo()
        } else {
            await step(undo: false, core: core, fromMenu: false)
        }
    }

    /// `fromMenu`: the window's undo manager is performing the step and registers its opposite itself; otherwise its
    /// stack is rebuilt from the history afterwards.
    private func step(undo: Bool, core: VixenCore, fromMenu: Bool) async {
        guard undo ? canUndo : canRedo else {
            notice = undo ? undoHelp : redoHelp
            if fromMenu {
                window?.undoManager?.removeAllActions(withTarget: undoTarget)
                history = ""
            }
            return
        }
        let label = undo ? undoLabel : redoLabel
        if await run(undo ? "display_undo" : "display_redo", [:], core: core, history: fromMenu ? .note : .rebuild) != nil, let label {
            status = "\(undo ? "Undid" : "Redid") \(StudioPreviewsModel.pretty(label))."
        } else {
            window?.undoManager?.removeAllActions(withTarget: undoTarget)
            history = ""
        }
    }

    /// The window's Edit ▸ Undo walks the shared history while this pane shows. The Display pane rebuilds its own steps
    /// when it comes back.
    func takeOverUndo() {
        window?.undoManager?.removeAllActions()
        history = ""
        if let core = StudioPreviewsCore.current {
            Task { await refreshHistory(core: core, mode: .rebuild) }
        }
    }

    func releaseUndo() {
        window?.undoManager?.removeAllActions(withTarget: undoTarget)
        history = ""
    }

    enum HistoryMode {
        /// This pane's own edit: add its one step.
        case register
        /// The window's undo manager just performed a step and already holds its opposite.
        case note
        /// Someone else changed the history: rebuild every step.
        case rebuild
    }

    private func refreshHistory(core: VixenCore, mode: HistoryMode) async {
        let text = await Task.detached { core.studioDisplayHistory() }.value
        applyHistory(text, mode: mode)
    }

    /// Mirrors the shared history into the window's undo manager, so Edit ▸ Undo reaches it.
    private func applyHistory(_ text: String, mode: HistoryMode) {
        guard let decoded = try? JSONDecoder().decode(StudioScheduleHistory.self, from: Data(text.utf8)) else { return }
        undoLabel = decoded.undo.first
        redoLabel = decoded.redo.first
        undoNoGraph = decoded.undoPreviewOnly
        redoNoGraph = decoded.redoPreviewOnly
        guard paneVisible, text != history, let manager = window?.undoManager else { return }
        history = text
        guard manager.groupingLevel == 0, !manager.isUndoing, !manager.isRedoing else {
            history = ""
            return
        }
        switch mode {
        case .note:
            return
        case .register:
            if let label = decoded.undo.first {
                ownGroup(manager) { registerStep(manager, label: label, undoing: true) }
            }
            return
        case .rebuild:
            break
        }
        manager.removeAllActions(withTarget: undoTarget)
        for label in decoded.undo.reversed() {
            ownGroup(manager) { registerStep(manager, label: label, undoing: true) }
        }
    }

    private func ownGroup(_ manager: UndoManager, _ register: () -> Void) {
        let automatic = manager.groupsByEvent
        manager.groupsByEvent = false
        manager.beginUndoGrouping()
        register()
        manager.endUndoGrouping()
        manager.groupsByEvent = automatic
    }

    private func registerStep(_ manager: UndoManager, label: String, undoing: Bool) {
        manager.registerUndo(withTarget: undoTarget) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let core = StudioPreviewsCore.current else { return }
                self.registerStep(manager, label: label, undoing: !undoing)
                Task { await self.step(undo: undoing, core: core, fromMenu: true) }
            }
        }
        manager.setActionName(StudioPreviewsModel.pretty(label))
    }

    // MARK: Saving

    /// The profile saves right after each change, one save at a time, so nothing waits for the pane to be left.
    private func scheduleSave(core: VixenCore) {
        guard saveTask == nil else {
            saveAgain = true
            return
        }
        status = "Saving…"
        saveTask = Task {
            repeat {
                saveAgain = false
                let failure: String? = await Task.detached {
                    do {
                        _ = try core.studioDisplayCommand(command: "profile_save", argumentsJson: #"{"confirm":true}"#)
                        return nil
                    } catch {
                        return StudioScheduleFailure(error).text
                    }
                }.value
                if let failure {
                    notice = "Couldn't save: \(failure)"
                }
            } while saveAgain
            saveTask = nil
            if notice == nil || notice?.hasPrefix("Couldn't save") == false {
                status = status == "Saving…" ? "All changes saved" : status
            }
        }
    }

    /// Waits for a save in flight. The app awaits it before shutting down.
    func flush() async {
        await saveTask?.value
    }
}

/// The way to the section for Scheduler ▸ Shows and Schedules. Set by the show window.
@MainActor
enum StudioScheduleCore {
    static var reveal: (() -> Void)?
}

private struct StudioScheduleFailure: Error, Sendable {
    let text: String
    nonisolated init(_ error: Error) {
        if case FfiError.Failed(let message) = error { text = message } else { text = "\(error)" }
    }
}

/// Tells the model which window hosts the pane, for its undo manager.
private struct StudioScheduleWindowReader: NSViewRepresentable {
    final class Marker: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window {
                MainActor.assumeIsolated {
                    StudioScheduleModel.shared.window = window
                    StudioScheduleModel.shared.takeOverUndo()
                }
            }
        }
    }

    func makeNSView(context: Context) -> Marker { Marker() }
    func updateNSView(_ marker: Marker, context: Context) {}
}
