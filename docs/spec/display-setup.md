# Display Setup

## Scope

Upstream paths are relative to `upstream/Vixen/src/`. Line counts are from `wc -l`. Every file listed was read in full unless it is under "Read in part" or says otherwise. In the entries below, a bare `:n` line reference continues the most recently named file, and `Designer :n` means that file's `.Designer.cs`; when a sentence names a helper file in passing and then goes back to the main one, the main file is named again.

**Application/Setup, window shell:** Vixen.Application/Setup/DisplaySetup.cs 281, DisplaySetup.Designer.cs 318, ISetupElementsControl.cs 33, ISetupPatchingControl.cs 31, ISetupControllersControl.cs 55, ElementsChangedEventArgs.cs 22. Vixen.Application/VixenApplication.cs lines 958-1036 and 1252-1270 only (how the window is opened and what happens on close).

**Application/Setup, panels:** SetupElementsTree.cs 513, SetupElementsTree.Designer.cs 398, SetupPatchingSimple.cs 698, SetupPatchingSimple.Designer.cs 797, SetupControllersSimple.cs 302, SetupControllersSimple.Designer.cs 375.

**Vixen.Common/Controls:** ElementTree.cs 1580, ElementTree.Designer.cs 346, MultiSelectTreeview.cs 1397, ControllerTree.cs 1367, ControllerTree.Designer.cs 228, MessageBoxForm.cs 116, TextDialog.cs 50, NumberDialog.cs 35, ListSelectDialog.cs 93 (the last four for the shared prompts' behaviour), TextDialog.Designer.cs and ListSelectDialog.Designer.cs for the window titles.

**Vixen.Common/Utilities:** ElementTemplateHelper.cs 150. **Also:** Vixen.Common/Controls/BaseForm.cs 14 (only sets a font), Vixen.Common/Help/Help.cs lines 50-54 (help URLs), Vixen.Core/Module/OutputFilter/OutputFilterModuleInstanceBase.cs lines 92-104 (a filter's title is its type name unless overridden, as the Tagged filter does).

**Vixen.Core/Services:** ApplicationServices.cs 193, ElementNodeService.cs 68 (`CreateMultiple`, `ImportTemplateOnce`, `ImportTemplateMany` and `Rename` have no callers anywhere), ElementTagService.cs 191, OutputFilterService.cs 15 (an empty singleton), FixtureSpecificationService.cs 21 (only loads a profile file).

**Vixen.Core/Sys/Managers:** NodeManager.cs 233, ElementManager.cs 218, FilterManager.cs 221, DataFlowManager.cs 237, OutputControllerManager.cs 246.

**Application/Setup, graphical patching:** SetupPatchingGraphical.cs 2101, SetupPatchingGraphical.Designer.cs 271. **Application/GraphicalPatching:** ConnectionTool.cs 1596, FilterSetupShapeBase.cs 351, FilterShape.cs 88, NestingSetupShape.cs 56, OutputShape.cs 94, ElementNodeShape.cs 79, ControllerShape.cs 41, DataFlowConnectionLine.cs 47, NShapeLibraryInitializer.cs 36. NShape itself (Vixen.Common/NShape, NShapeWinFormsUI) is out of scope and was consulted only for defaults, key handling and history: Display.cs lines 854-870, 942-960, 1012-1018, 1860-1935, 2379-2420, 2582-2790, 4445-4460, 5623-5655, 6128-6223; NShape/Tool.cs lines 169, 212-253; NShape/DiagramSetController.cs lines 221-257; NShape/Project.cs lines 428-435; NShape/History.cs lines 296-365; NShape/ShapeCollection.cs lines 1259-1364.

**Application/Setup/ElementTemplates:** ElementTemplateBase.cs 48, StartLocation.cs 10, SingleItem.cs 83, NumberedGroup.cs 104, NumberedGroup.Designer.cs 203, Arch.cs 17, CandyCane.cs 17, Star.cs 17, StarBurst.cs 116, StarBurst.Designer.cs 267, Icicles.cs 147, Icicles.Designer.cs 237, LipSync.cs 70, LipSync.Designer.cs 113, Megatree.cs 212, Megatree.Designer.cs 353, PixelGrid.cs 285, PixelGrid.Designer.cs 421, IntelligentFixtureTemplate.cs 1377. Vixen.Modules/Property/Order/OrderModule.cs (for `AddPatchingOrder`), Vixen.Core/Utility/NamingUtilities.cs, Vixen.Core/Sys/ElementTagCollection.cs 91.

**Vixen.Common/Controls/NameGeneration:** NameGenerator.cs 443, SubstitutionRenamer.cs 299, NumericCounterEditor.cs 61, LetterCounterEditor.cs 48, LetterIteratorEditor.cs 29, WordIteratorEditor.cs 29, NameGeneratorEditor.cs 12. Their Designer files via docs/reference/forms/Vixen.Common__Controls__NameGeneration__*.json, plus the defaults in NameGenerator.Designer.cs 638 lines 161-183, 560-566 and SubstitutionRenamer.Designer.cs 353 lines 315-319. **Vixen.Core/Rule:** INamingGenerator.cs, INamingTemplate.cs, Name/NumberedNamingTemplate.cs 31, Name/NumericCounter.cs 76, Name/LetterCounter.cs 89, Name/LetterIterator.cs 56, Name/WordIterator.cs 48.

**Vixen.Common/ElementTagManager:** ElementTagManager.csproj 20, Views/ElementTagManagerWindow.xaml 52, Views/ElementTagManagerWindow.xaml.cs 51, ViewModels/ElementTagManagerWindowViewModel.cs 93, ViewModels/TagColorItem.cs 98.

**Vixen.Modules/App/Modeling:** WireDiagramSvgBuilder.cs 445, WireDiagramExportOptions.cs 60, ViewModels/WireDiagramExportOptionsViewModel.cs 239, Views/WireDiagramExportOptionsWindow.xaml 123, Views/WireDiagramExportOptionsWindow.xaml.cs 60.

**Vixen.Modules/Editor/FixtureWizard/Wizard** (Intelligent Fixture's dialog): Models/IntelligentFixtureWizard.cs 222, SelectProfileWizardPage.cs 169, GroupingWizardPage.cs 74, AutomationWizardPage.cs 90, ColorSupportWizardPage.cs 71, DimmingCurveWizardPage.cs 112, DimmingCurveSelection.cs 23, EditProfileWizardPage.cs 22, EditProfileFunctionsWizardPage.cs 22, FixtureWizardPageBase.cs 33, SummaryWizardPageBase.cs 50, Interfaces/IFixtureWizard.cs 41; ViewModels/SelectProfileWizardPageViewModel.cs 348, GroupingWizardPageViewModel.cs 296, AutomationWizardPageViewModel.cs 137, ColorSupportWizardPageViewModel.cs 67, DimmingCurveWizardPageViewModel.cs 101, EditProfileWizardPageViewModel.cs 152, EditProfileFunctionsWizardPageViewModel.cs 90, EditWizardPageViewModelBase.cs 63, IIntelligentFixtureWizardPageViewModel.cs 20, FixtureWizardTreeItem.cs 51; FixtureWizardNavigationController.cs 110; Views/*.xaml (SelectProfile 102, Grouping 113, Automation 65, ColorSupport 69, DimmingCurve 61, EditProfile 15, EditProfileFunctions 16), WizardUserControlBase.cs 68, and the code-behind that only calls `DrawMovingHeads`.

**Vixen.Core, serialisation of the export:** Sys/ElementNodeProxy.cs 25, the `ElementProxy` class, IO/JSON/ElementTreeWriter.cs 42, IO/JSON/ElementTreeReader.cs 37, IO/JSON/CustomContrractResolver.cs.

**Vixen.Tests** (read for expected values, not in full unless noted): Setup/SetupPatchingSimpleElementOrderTests.cs 80 in full; Setup/SetupPatchingSimpleOutputOrderTests.cs 342 and Common/ControllerTreeVirtualizationTests.cs 400 for the cases quoted; test names for Common/MultiSelectTreeview*Tests.cs, Core/OutputControllerOutputIndexTests.cs, Services/ElementTagServiceTests.cs, App/Modeling/*Tests.cs. **Design notes:** upstream/Vixen/docs/plans/display-setup/display-setup-ok-performance.md, vix-4004, vix-4006, vix-4009, vix-938 (purpose sections), vix-4008 (first part).

**Read in part, only what this area touches:** Vixen.Core/Sys/VixenSystem.cs (85-125, 145-150, 161-200, 230-342); Vixen.Core/Sys/Managers/OutputDeviceCollectionExecutionMediator.cs 148 (85-122), OutputDeviceExecution.cs 254 (15-100); Vixen.Modules/App/Fixture/FixtureSpecification.cs 499 (94-102, 133-226, 266-294, 362-404); Vixen.Modules/App/FixtureSpecificationManager/FixtureSpecificationManager.cs 266 (188-228); Vixen.Modules/Property/IntelligentFixture/IntelligentFixtureModule.cs 119 (only its Setup entry); Vixen.Common NShape as stated above; `HelpStrings` for the help URL.

**In scope but not read:** `.resx` files, which hold designer icon streams and no behaviour. The Orc.Wizard and Catel libraries behind the wizard are NuGet packages with no source in the repo, so how `FastForwardNavigationController` (the base of FixtureWizardNavigationController.cs:10) enables Finish on pages before Summary could not be read (F139). The form dumps in docs/reference/forms for Setup and for ElementTree, ControllerTree, TextDialog, NumberDialog, ListSelectDialog, MessageBoxForm and the NameGeneration controls were checked control by control against this spec. **Outside this spec, by intent:** the fixture profile editors (Editor/FixturePropertyEditor, hosted by two wizard pages), the per-property and per-filter and per-controller dialogs (docs/spec/property-filter-controller-setup.md), and NShape's internals. Vixen.Core/Services files unrelated to elements (Sequence, Media, Timing and so on) and the other files under Sys/Managers (context, preview, smart controller managers) were not read as nothing here calls them beyond what is listed.

**Vixen.Core, supporting:** Sys/ElementNode.cs 331, Sys/GroupNode.cs 140, Sys/PropertyManager.cs 140, Sys/Output/OutputController.cs 399, Sys/ElementTagDefinition.cs 76, Sys/BuiltInElementTags.cs 79, Data/Flow/CommandOutputDataFlowAdapter.cs 45, Data/Flow/ElementDataFlowAdapter.cs 77, Data/Flow/DataFlowComponentReference.cs 47, Data/Flow/DataFlowPatch.cs 45, Rule/IElementTemplate.cs 59, Rule/IElementSetupHelper.cs 19. **Vixen.Modules/App/Modeling:** ElementModeling.cs 128.

## Features

Conventions used in this section. "Live edit" means the change is applied straight to the in-memory system model and only becomes permanent when the window closes with OK (F6). No model edit inside Display Setup has an undo step: the upstream window has no Undo menu, and the one Undo that exists (Ctrl+Z on the Graphical canvas, F118) only affects the picture, never the model. The only way back is Cancel, which throws away everything done since the window opened (F7). The Mac version keeps that whole-window Cancel, and the undo notes below say "Cancel only" where that is the case. MessageBoxForm's Yes button returns `DialogResult.OK`, No returns `DialogResult.No`, Cancel returns `DialogResult.Cancel` (Vixen.Common/Controls/MessageBoxForm.Designer.cs:46,58,70); code below that checks for "OK" on a Yes/No box means "Yes". The two-boolean constructor is `(text, title, noVisible, cancelVisible)`, and when either extra button is visible the OK button reads "YES" (MessageBoxForm.cs:11-25). The Mac uses `.alert` with upstream's title and text and verbs on the buttons (see the structure section).

Numbers are stable, so entries added after the first pass (F149 onward) sit in the section they belong to, out of numeric order.

**Persistence:** every model edit below is held in memory until OK writes it to the system and module configuration (F6), or Cancel discards it (F7), unless the entry says otherwise. Panel state (selection, expansion, radios, checkboxes, combos, the active patching view) is never saved. **Mac:** Return is the default button and Esc cancels on every sheet, as in docs/spec/property-filter-controller-setup.md. Changes from upstream are collected in section N.

### A. Display Setup window

- [ ] **F1. Open Display Setup.** The user clicks the main window button `buttonSetupDisplay` or the menu System Configuration > "Setup Display" (Vixen.Application/VixenApplication.cs:1252-1270, Designer :159). Both call `SetupDisplay()`, which shows `DisplaySetup` modally with `await form.ShowDialogAsync()` (VixenApplication.cs:991-996). Both entry points are disabled together while `EnableButtons(false)` is in effect, which also disables New Sequence, Open Sequence, Setup Previews and the whole main menu strip (VixenApplication.cs:461-466). Title "Display Setup", start position centre of parent, client size 1281 x 737, `AutoSize`, `DoubleBuffered` (DisplaySetup.Designer.cs:276-287). OK is the Accept button and Cancel the Cancel button (Designer :276,280), so Return means OK and Escape means Cancel while focus is not in a control that consumes them.

- [ ] **F2. Three-column layout.** One row, three columns at 25% / 50% / 25%: Elements left, Patching centre, Controllers right, with an outset double cell border (DisplaySetup.Designer.cs:258-265). Every column has a 12pt centred header label over a one-line divider: "Elements", "Patching", "Controllers" (Designer :92, :126, :202; font set at DisplaySetup.cs:26-31). Below that divider the Elements column holds the elements panel (row 2 at 100%, DisplaySetup.cs:76), the Controllers column holds the controllers panel (row 2 at 100%, DisplaySetup.cs:186) with a 40pt fixed row 3 under it for the OK/Cancel/Help buttons (Designer :171,:180), and the Patching column holds the view radio buttons (row 2) above the active patching view (row 3 at 100%, spanning both columns, DisplaySetup.cs:129-130).

- [ ] **F3. Patching view switch.** Radio buttons in the Patching header: left "Simple Patching" (`radioButtonPatchingSimple`), right "Graphical View" (`radioButtonPatchingGraphical`), each centred in a 50% column (Designer :102-103,:148,:161). Selecting one calls `ActivatePatchingControl`, which unsubscribes from the old view and removes it, then adds the new one and immediately pushes the current controller selection and then the current element selection into it (DisplaySetup.cs:109-151). The view that is not showing keeps its own state but receives no selection or details updates while inactive. On load the window always selects Simple: `radioButtonPatchingSimple.Checked = true` (DisplaySetup.cs:58). The choice is not persisted, so every open starts on Simple. Load order is controllers panel, then elements panel, then the radio (DisplaySetup.cs:55-58).

- [ ] **F4. Cross-panel event wiring.** These rules must hold whatever the implementation looks like (DisplaySetup.cs:82-105, 153-164, 191-223):
  - Element selection changed: the active patching view gets `UpdateElementSelection(selected)`.
  - Elements changed (any action): the active patching view gets `UpdateElementDetails(selected)`. If the action was Remove or Edit, the controllers panel also refreshes its patch status (`UpdatePatching`). Add and Rename do not refresh the controllers panel by themselves. **But every event coming out of the tree control arrives as Edit**, whatever it carried: `elementTree_ElementsChanged` always refills the Selected Item(s) group and raises Edit (SetupElementsTree.cs:361-365). So only the panel's own explicit raises are Add (F10), Remove (F12) and Rename (F13), and in practice only the Rename button skips the controllers refresh, since Add's `AddNodePathToTree` has already raised Edit (ElementTree.cs:852).
  - Controller selection changed: the active patching view gets `UpdateControllerSelection(selected)`.
  - Controllers changed: the active patching view gets `UpdateControllerDetails(selected)` and the elements panel refreshes its status icons (`UpdatePatching`).
  - Patching updated (raised by a patching view): both the elements panel and the controllers panel refresh status.
  - `FiltersAdded` is subscribed but its handler is empty (DisplaySetup.cs:166-168). Not user-visible.
  - A selection set from outside (F5) reaches the patching view through ElementsChanged(Edit) for the element tree (ElementTree.cs:187-192), so it also refreshes the controllers panel, and through ControllerSelectionChanged for the controller tree (ControllerTree.cs:81-86).
  - The patching view only ever receives these events while it is the active one: `ActivatePatchingControl` unsubscribes the old view's `FiltersAdded` and `PatchingUpdated` (DisplaySetup.cs:111-116), which is also why F126's tree icons go stale.

- [ ] **F5. Cross-panel selection commands.** `SelectElements(elements, updateScrollPosition)` sets the element tree's selection (repopulating it) and optionally scrolls the first selected row to the top; `SelectControllersAndOutputs(set, updateScrollPosition)` does the same for the controller tree (DisplaySetup.cs:254-268). These are the targets of the two "find" buttons F14 and F84. `control_UnpatchControllers` is the target of the controller tree's two Unpatch context items: it always runs the Simple view's unpatch code, even when the Graphical view is showing, and then refreshes the graphical view's connections for those controllers if that view is the active one (DisplaySetup.cs:210-215).

- [ ] **F6. OK.** Bottom right of the Controllers column, in the order OK, Cancel, Help, each 89 x 29 (Designer :207-254). Closes with `DialogResult.OK`. Then, on the main window with a wait cursor, buttons disabled and a progress bar (VixenApplication.cs:998-1009, 1030-1035):
  1. progress 0 "Preparing Display Setup Changes": `ApplyConfirmedChanges()`, which removes orphaned filters (`VixenSystem.Filters.RemoveOrphanedFilters()`), removes orphaned properties (`PropertyManager.RemoveOrphanedProperties()`) and reorders controllers (`ControllerTree.ReorderControllers`) (DisplaySetup.cs:270-278; the comment says it exists because the graphical designer leaves unconnected filters behind);
  2. progress 50 "Saving System Configuration": `VixenSystem.SaveSystemConfigAsync()`;
  3. progress 100 "Saving Module Configuration": `VixenSystem.SaveModuleConfigAsync()`.
  The save is the only point at which anything done in the window reaches disk, with these exceptions: the Intelligent Fixture wizard writes the fixture profile file when it finishes, and Cancel does not undo that (F110); Export Element Tree (F53) and Export Wire Diagram (F131) write their own files.

- [ ] **F7. Cancel, and closing the window.** Any non-OK result (Cancel button, Escape, the window close box) runs `VixenSystem.ReloadSystemConfig()` under "Reloading System Configuration", then brings the main window to the front (VixenApplication.cs:1011-1016). There is no "discard changes?" prompt. This is the upstream's only undo. All live edits are thrown away, including new elements, patches, filters and controllers. The reload closes execution, reloads, and reopens execution if it was open (Vixen.Core/Sys/VixenSystem.cs:334-342), so controller run states go back to their saved state. See F91.

- [ ] **F8. Help.** Button "Help" with a help icon opens the Display Setup help topic via `VixenHelp.ShowHelp(HelpStrings.SetupMain)`, which is `https://www.vixenlights.com/docs/usage/display-setup/` (DisplaySetup.cs:249-252; Vixen.Common/Help/Help.cs:50-51). On the Mac: open the same topic URL in the default browser, and also expose it from the Help menu.

- [ ] **F150. Escape while something is in progress.** Upstream, Esc belongs to the form's Cancel button (F1). The canvas does not ask for the key (`Display` has no `IsInputKey` override), so `Form.ProcessDialogKey` takes it before NShape's tool ever sees it (NShape/Tool.cs:212-223 is never reached). So pressing Esc in the middle of a connection, move or frame drag on the canvas closes Display Setup and discards every edit (F7). A tree drag is the exception, because the system drag loop consumes Esc and `OnKeyUp` restores the row colours (MultiSelectTreeview.cs:867-886). This follows from how WinForms routes dialog keys; it was read from the code, not seen running. **Mac:** Esc during any drag in either tree or on the canvas cancels only that drag, with the window left open and the model untouched. Esc with nothing in progress and no sheet open is Cancel (F7).

### B. Elements panel (left column)

Layout top to bottom: Add row, element tree (fills the height), row of tree buttons, "Selected Item(s):" group (SetupElementsTree.Designer.cs:312-329).

- [ ] **F9. "Add:" type picker.** Label "Add:", then a drop-down list `comboBoxNewItemType` holding every element template by `TemplateName`, in the order discovered by reflection (SetupElementsTree.cs:44-57; ApplicationServices.cs:125-134). Default selection is "Single Item" if present, else the first (SetupElementsTree.cs:49-57). Reflection order is not meaningful, so the port fixes this order explicitly; see F99. The selection is not persisted between opens.

- [ ] **F10. Add Elements (+) button.** Tooltip "Add Elements" (Designer :99), to the right of the combo. Enabled when a template is selected (`SelectedIndex >= 0`, SetupElementsTree.cs:349). Click: the button disables itself against double-clicks, runs the shared template flow (F100) against the current tree selection, re-enables itself (SetupElementsTree.cs:267-301). The flow calls back once, with the first created node (the head): add its row, select it, raise ElementsChanged(Add), refresh the selected-item group, scroll the selection to the top (SetupElementsTree.cs:288-295). Undo: Cancel only.

- [ ] **F11. Element tree.** See section C. `AllowDragging`, `AllowPropertyEdit` and `AllowWireExport` are all true here (SetupElementsTree.Designer.cs:333-335). Selecting (F26's AfterSelect) or deselecting anything refreshes the group below and raises ElementSelectionChanged carrying the elements in selection order (SetupElementsTree.cs:378-388).

- [ ] **F12. Delete Elements button.** Tree button row, first, tooltip "Delete Elements" (Designer :266). Enabled when at least one element is selected (SetupElementsTree.cs:356). Prompts "Are you sure you want to delete these element(s)?", title "Delete Elements?", Yes/No, question icon (SetupElementsTree.cs:478-483). On Yes: deletes each selected tree row (so a node that appears in several groups is removed only from the parent the selected row is under, see F44), repopulates, raises ElementsChanged(Remove) (SetupElementsTree.cs:486-492). Undo: Cancel only.

- [ ] **F13. Rename Elements button.** Second in the row, tooltip "Rename Elements" (Designer :278). Enabled when at least one selected (SetupElementsTree.cs:357). Calls `RenameSelectedElements` (F49), and raises ElementsChanged(Rename) when it returns true (SetupElementsTree.cs:495-501).

- [ ] **F14. Find outputs button.** Third in the row, tooltip "Find outputs these elements are patched to" (Designer :254). Enabled when at least one selected (SetupElementsTree.cs:358). Algorithm (SetupElementsTree.cs:410-471): take each selected node's leaves, de-duplicated by element id, skipping leaves with no element; push each element's data flow component on a stack; depth-first through `GetDestinationsOfComponent`, with a visited set by `DataFlowComponentId`; every `CommandOutputDataFlowAdapter` reached is turned into (controller, output index) via `OutputControllers.getOutputDetailsForDataFlowComponent` and added to a `ControllersAndOutputsSet`, skipping when the controller is null. Empty result shows "No controller patch points found.", title "Not Found", information icon. Otherwise selects those outputs in the controller tree and scrolls to them (`SelectControllersAndOutputs(set, true)`). No change to data.

- [ ] **F15. "Selected Item(s):" group: Configure row.** Label "Configure:", drop-down `comboBoxSetupHelperType` listing every `IElementSetupHelper` by `HelperName`, in reflection order, default first (SetupElementsTree.cs:59-67). The button to its right has a cog-go icon and tooltip "Configure" (Designer :212). The button is enabled only when at least one element is selected: `UpdateButtons` first sets it from the combo index, then overwrites it with "any selected" (SetupElementsTree.cs:348,352). Click runs `helper.Perform(selected)` (unordered selection, `SelectedElementNodes`), then refreshes tree status icons and the group, then raises ElementsChanged(Edit) whatever the helper returned (SetupElementsTree.cs:130-144). The helpers themselves, and their fixed Mac order, are in docs/spec/property-filter-controller-setup.md (its F4). Undo: Cancel only.

- [ ] **F16. "Selected Item(s):" group: Properties list.** Label "Properties:", one-column list `listViewProperties` with no header, showing each property's `Descriptor.TypeName`, in the order of `selectedNode.Properties`, for the **single** node `elementTree.SelectedNode` (the tree's primary node, not the whole selection; the TODO at SetupElementsTree.cs:312-313 notes this) (SetupElementsTree.cs:309-331). The list clears its selection after each refill. The column auto-sizes to content but never wider than the list, and clamps to 94% of the list width when content is wider (SetupElementsTree.cs:333-344). The list allows multi-select (the default for ListView). Double-click anywhere in the list configures the property (F19) when exactly one row is selected (SetupElementsTree.cs:390-393). The three property buttons sit under the list, left to right: Add, Delete, Configure (SetupElementsTree.Designer.cs x positions 84, 119, 154).

- [ ] **F17. Add Property (+).** Tooltip "Add Property" (Designer :169). Enabled when at least one element is selected (SetupElementsTree.cs:353). Opens `ListSelectDialog("Add Property", <every property module by TypeName>)` in single-select mode (SetupElementsTree.cs:148-156). Upstream's title bar actually reads "Select Item" (F148). On OK: `node.Properties.Add(typeId)` on every selected tree row's node, not their descendants (:161-164; TODO at :159). `Properties.Add` behaviour for a property the node already has is defined by PropertyManager; see the Data section. Then refresh the group and raise ElementsChanged(Edit). Undo: Cancel only.

- [ ] **F18. Delete Property.** Tooltip "Delete Property" (Designer :156). Enabled when the list has rows and at least one is selected (SetupElementsTree.cs:354). Prompt: one row selected: "Are you sure you want to remove the selected property from the element?", title "Remove Property?"; several selected: "Are you sure you want to remove the selected properties from the element?", title "Remove Properties?". Buttons YES and Cancel, error icon (constructor `(…, false, true)`, SetupElementsTree.cs:176-190). On Yes: for every selected row and every selected element, `elementNode.Properties.Remove(typeId)` (:193-203), even for elements that were not the node the list was filled from. Refresh, raise ElementsChanged(Edit). Undo: Cancel only.

- [ ] **F19. Configure Property.** Tooltip "Configure Property" (Designer :143). Enabled when the list has rows and exactly one is selected (SetupElementsTree.cs:355). `ConfigureSelectedProperties` (SetupElementsTree.cs:217-265) branches on the property in this order:
  1. It is the Color property: build a `ColorSetupHelper`, preload it with the property's `ColorType`, `ColorSetName` and `SingleColor`, run `helper.Perform(SelectedElements)` across all selected elements, raise ElementsChanged(Edit) unconditionally. The upstream comment explains that this is what makes colour edits update colour breakdown patching. `result` stays false.
  2. `property.HasSetup`: run `property.Setup()`. If it returns true, `CloneValues` the configured property into every other selected element that has this property type and a different `ModuleData`; raise ElementsChanged(Edit).
  3. `property.HasElementSetupHelper`: `property.SetupElements(SelectedElements)`; if true, raise ElementsChanged(Edit).
  A property with neither does nothing. Per-property dialogs are specified in docs/spec/property-filter-controller-setup.md.

- [ ] **F20. Enable rules for the elements panel, all together** (`UpdateButtons`, SetupElementsTree.cs:346-359; re-run on combo change, list selection change, tree selection, and after each action):

  | Control | Enabled when |
  |---|---|
  | Add Elements (+) | a template is selected |
  | Configure (run helper) | any element selected |
  | Add Property | any element selected |
  | Delete Property | property rows exist and at least one selected |
  | Configure Property | property rows exist and exactly one selected |
  | Delete Elements | any element selected |
  | Rename Elements | any element selected |
  | Find outputs | any element selected |

### C. Element tree control (`Common.Controls.ElementTree`)

This control is also used elsewhere in the app (for example Preview setup), hence its three "Allow" switches (ElementTree.cs:77-87). The Mac tree should be one reusable component.

- [ ] **F21. Rows and hierarchy.** Roots are `VixenSystem.Nodes.GetRootNodes()` in order; children are `ElementNode.Children` in order (ElementTree.cs:140-143, :348). The same node can appear under several parents, so a row's identity is its **path**, a sequence of node GUIDs joined by the tree's path separator, not the node id (ElementTree.cs:222-233, :319). Row text is the element name, drawn with `NoPrefix`, so "&" shows literally (ElementTree.cs:426).

- [ ] **F22. Status icon per row** (`UpdateTreeNodeImage`, ElementTree.cs:384-405). A node with children: "Group" folder icon. A leaf: if it has an element and its data flow component has at least one destination, then red ball when `Element.Masked`, green ball otherwise; every other leaf: white ball. The image list also defines GreyBall, YellowBall and BlueBall (Designer :72-78), which the element tree does not use. Refresh of all materialised rows happens on `RefreshElementTreeStatus` (:360-382).

- [ ] **F23. Lazy children.** A group row gets a single placeholder child named "VIRT" until first expanded; `BeforeExpand` swaps in the real children, one level at a time, and re-expands any child whose path is remembered (ElementTree.cs:48-59, :334-358). On the Mac this is natural with `NSOutlineView` data-source laziness, and the placeholder must not exist.

- [ ] **F24. Expansion memory.** `_expandedNodes` is a set of paths kept for the life of the control. Added in `BeforeExpand`, removed in `AfterCollapse` (ElementTree.cs:43-59). Repopulating restores expansion and selection and the top visible row (trying a list of candidates in order, in case the top one was deleted) (:125-193, :298-314). Not persisted across window opens. "Collapse All" (F48) collapses everything and so empties the memory, because each collapse fires `AfterCollapse`.

- [ ] **F25. Tag colour dots.** To the right of each row's text, one 8pt dot per tag on that node having a non-empty `DisplayColor`, sorted by the tag's `SortOrder`, 3pt apart, starting 6pt after the text (ElementTree.cs:411-467). Colour is an HTML colour string. Unknown tag ids are skipped.

- [ ] **F26. Selection.** Multi-select, with behaviours from MultiSelectTreeview (section E). `SelectedElementNodes` is in display order; `SelectedElementNodesInSelectionOrder` is anchor-to-endpoint during a range gesture, which is what patching order uses (ElementTree.cs:488-501; SetupElementsTree.cs:97-108). `HideSelection` is false, so the selection stays visible when the tree loses focus (ElementTree.Designer.cs:271).

- [ ] **F27. Selecting from code.** `PopulateNodeTree(elements)` builds each element's path by following `Parents.FirstOrDefault()` up to the root (so a multi-parent node always resolves to its first parent's path; upstream calls this logic flawed, VIX-3573, ElementTree.cs:235-254), rebuilds the tree, expands the ancestors of each selected path and selects it, then raises ElementsChanged (ElementTree.cs:97-105, :155-162, :187-192, :195-213). `UpdateScrollPosition` puts the first selected row at the top (:119-123). Keep first-parent selection: it only decides which row of a multi-parent node gets highlighted.

- [ ] **F28. Keyboard: Delete** (ElementTree.cs:1504-1523). When at least one row is selected: prompt "Are you sure you want to delete these element(s)?", title "Delete Elements?", YES/No, question icon; on Yes delete each selected row, repopulate, raise ElementsChanged. On the Mac: both Delete (backspace) and forward delete, using ⌘⌫ too, per Finder habit.

- [ ] **F29. Keyboard: Copy, Paste, Paste as New.** Ctrl+C copies, Ctrl+V pastes, Ctrl+Shift+V pastes as new (ElementTree.cs:1524-1538; menu shortcuts Designer :91-116). Mac: ⌘C, ⌘V, ⇧⌘V, through the Edit menu when the tree has focus. Ctrl+X is commented out at :1540-1544 but see F33.

- [ ] **F30. Keyboard: tree navigation.** Handled by MultiSelectTreeview; see F73.

- [ ] **F31. Find elements (controllers side).** Described at F84 because the button lives in the controllers panel.

- [ ] **F149. Double-click on an element row.** The element tree registers no double-click handler (the Designer wires only `KeyDown`, ElementTree.Designer.cs:282), so the Windows default applies: double-clicking a row with children toggles it open or closed. The controller tree has to cancel exactly this default (ControllerTree.cs:1278-1294), which is how we know it is on here. Read from the code, not seen running. No data change, and it feeds the same expansion memory (F24). A leaf does nothing. `NSOutlineView` has no such default, so **Mac:** set `doubleAction` to toggle the clicked row's expansion.

### Context menu on the element tree

Order exactly as `Items.AddRange` lists it (ElementTree.Designer.cs:83): Cut, Copy, Paste, Paste as New, separator, Item properties >, separator, Add, Add Multiple, Delete, Create Group, Rename, Find/Replace, Collapse All, Reverse Elements, Sort, Export Wire Diagram >, Export Element Tree, separator, Tags >. Enable and visibility rules are recomputed each time it opens (ElementTree.cs:982-1006). Keep this exact wording and order in `.contextMenu` / `NSMenu`. Access keys (`&Add`, `Add &Multiple`, `&Delete`, `Create &Group`, `Re&name`, `&Collapse All`, `&Sort`) have no meaning on the Mac.

- [ ] **F32. Enable and visibility table** (ElementTree.cs:986-1005):

  | Item | Rule |
  |---|---|
  | Cut | always disabled |
  | Copy | at least one selected |
  | Paste | `CanPaste()` |
  | Paste as New | `CanPaste(true)` |
  | Item properties | visible if `AllowPropertyEdit`; enabled when at least one selected |
  | Copy properties | exactly one selected |
  | Paste properties | at least one selected and a properties clipboard exists |
  | Add | always |
  | Add Multiple | no rule set, so always enabled |
  | Delete | at least one selected |
  | Create Group | at least one selected |
  | Rename | at least one selected |
  | Find/Replace | at least one selected |
  | Collapse All | no rule set, so always enabled |
  | Reverse Elements | more than one selected and `treeview.CanReverseElements()` (F72) |
  | Sort | exactly one selected and it is not a leaf (`CanSortSelected`, :1469-1476) |
  | Export Wire Diagram | visible if `AllowWireExport`; enabled when exactly one row selected and it has children (`CanExportDiagram`, :1119-1134) |
  | Export Element Tree | the tree has at least one root row |
  | Tags | at least one selected; submenu rebuilt at each open |

- [ ] **F33. Cut.** Always disabled when the menu is open, with the code comment "temporarily disable Cut function till we can keep the underlying Elements around" (ElementTree.cs:984-986). The item's Ctrl+X shortcut remains on the menu item (Designer :91), and because the Designer leaves it enabled until the first menu open, Ctrl+X in the focused tree can run `CutNodesToClipboard` before the menu has ever been opened (deleting each selected row and stashing the nodes, :1147-1161). That follows from how WinForms routes a context menu's shortcuts; it was read from the code, not seen running. It is an accident rather than a feature. On the Mac: keep the item visible and disabled and do not handle ⌘X in the tree.

- [ ] **F34. Copy.** Puts the selected rows' nodes into a private in-control clipboard `_clipboardNodes`. Not the system pasteboard; the upstream comment concedes this (ElementTree.cs:1136-1137, :1168-1178). Copying references, not data, so later edits to a node show up in what is pasted. Cleared when the window closes.

- [ ] **F35. Paste: links, not copies.** `AddChildToParent(node, destination)` for each clipboard node, so the *same* node gains another parent and shows at both places (ElementTree.cs:1236-1246). Destination is the primary selected row, or the root when nothing is selected. `CanPaste` false means the system "hand" sound and nothing else (:1225-1229). Before pasting: the loses-patches prompt (F43) with the destination. Then expand the destination row, repopulate, raise ElementsChanged (:1248-1252). Undo: Cancel only. **Upstream quirk, not to port:** the clipboard still holds a node deleted after it was copied. Paste relinks it as a leaf with no element and no properties, its children already gone, and it can never be patched; Paste as New makes a bare new leaf with its name. The port's clipboard holds ids, skips any that have been deleted on both paths, and gives the same beep as a refused paste if nothing is left.

- [ ] **F36. `CanPaste(asNew)` rule** (ElementTree.cs:1185-1212). False when the clipboard is null. The invalid set is `destination.InvalidChildren()` if there is a selected destination, `VixenSystem.Nodes.InvalidRootNodes` if there is no destination and this is a plain paste, and empty if there is no destination and this is Paste as New. False if any clipboard node is in the invalid set; otherwise true. An empty clipboard list (non-null) gives true.

- [ ] **F37. Paste as New: deep duplicate.** Same flow as F35 (hand sound when `CanPaste(true)` is false, the F43 prompt with the destination, expand the destination, repopulate, raise ElementsChanged; ElementTree.cs:1214-1254), but for each clipboard node `DuplicateNodes(node, destination)` (ElementTree.cs:1279-1325):
  - Create a new node under `destination` with the same name (uniquified by ElementNodeService), with an element when the source `IsLeaf`, cloning each property with `Properties.AddWithoutDefaults(clone)` (:1266-1277).
  - Then recurse through children, keeping a map from source leaf id to new leaf, per top-level node:
    - Leaf already in the map: link the existing new leaf to this new parent (`AddChildToParent`), so a leaf appearing twice inside the copied tree stays one element.
    - Leaf not in the map: create it, clone properties, record it.
    - Group: create with `isLeaf=false`, recurse if it has children. **Quirk: child groups do not get their properties cloned** (:1318-1322 has no `DuplicateNodeProperties` call, unlike the top node and leaves). A group reached twice is duplicated twice rather than linked.
  - Patches are not duplicated: the new leaves are unpatched (their new data flow components have no destinations). Tags are not duplicated: only properties are cloned.

- [ ] **F38. Item properties > Copy properties.** With exactly one row selected, stores references to that node's property instances in `_clipboardProperties` (ElementTree.cs:1327-1338). Since they are references, later edits to the source are seen at paste time. The properties clipboard survives further selection changes until the control is destroyed.

- [ ] **F39. Item properties > Paste properties.** For every selected node and every clipboard property: use the node's existing property of that type, else `Properties.Add(typeId)`; then `destination.CloneValues(source)` (ElementTree.cs:1340-1372). Properties the node has but the clipboard lacks stay as they are. A failure to create logs an error and continues. Raises ElementsChanged. Undo: Cancel only. Note that pasting Color properties this way does not run `ColorSetupHelper` (unlike F19), so colour breakdown filters are not updated.

- [ ] **F40. Add.** `AddSingleNodeWithPrompt(SelectedNode)` (ElementTree.cs:1374-1379, :775-797): `TextDialog("Element Name?")`. On OK: name defaults to "New Element" when the response is empty; `AddNewNode(name, parent != null, parent)`, which first does the loses-patches prompt (F43) and returns null if declined; then `AddNodePathToTree`. Upstream dereferences that null in `AddNodePathToTree` (:791, :822), a crash path on declining. The port treats declining as cancel. Uses only the primary selected row as parent; no selection means a root node. Raises ElementsChanged. New node name is uniquified by the node manager; see the naming rules in F138.

- [ ] **F41. Add Multiple.** `AddMultipleNodesWithPrompt(SelectedNode)` (ElementTree.cs:742-773, :1381-1386). `NameGenerator` dialog seeded with the selected row's text, or "NewName" when nothing is selected. On OK: creates one node per non-empty generated name with `skipPatchCheck=true` (so, **unlike Add, there is no loses-patches prompt**: a patched leaf silently becomes a group; :756-758). When nothing was created: "Could not create elements.  Ensure you use a valid name and try again." (empty title, OK only, error icon, :761-765). Then `AddNodePathToTree(results)`, which selects the new rows and scrolls to the last. The dialog is F133. The menu handler raises ElementsChanged even when the dialog was cancelled, because the returned list is empty rather than null (:1381-1386); nothing changed, so this is only a wasted refresh and the port does not repeat it.

- [ ] **F42. Delete.** Same prompt and flow as F28 from the context menu (ElementTree.cs:1388-1407).

- [ ] **F43. "Loses patches" prompt.** `CheckAndPromptIfNodeWillLosePatches(node)` (ElementTree.cs:873-894). When `node` is a leaf-with-element that has at least one data flow destination: message "Adding items to this element will convert it into a Group, which will remove any patches it may have. Are you sure you want to continue?", title "Convert Element to Group?", buttons YES, No, Cancel, question icon. Anything except Yes aborts the operation. Used by Add (F40), Paste and Paste as New (F35, F37) and drag-drop (F57). Not used by Add Multiple (F41), by Create Group (it only adds to the brand new group), or by the template flow (templates always create at the root, F99).

- [ ] **F44. Row deletion semantics.** `DeleteNode(row)` (ElementTree.cs:733-740): `VixenSystem.Nodes.RemoveNode(node, parentOfThisRow, true)`, so only this row's parent link goes; see `RemoveNode` in the Data section for what happens to orphaned nodes and elements. It also raises ElementsChanged once **per row**, before the final repopulate (:739), so deleting n rows costs n full recomputes of the patching view and n controller status refreshes. The port raises one event per gesture.
  **Upstream bug, not to port: a group row selected together with rows under it.** Rows are deleted in display order and the tree is not rebuilt between them. Deleting the group's row unlinks its children and forgets the group. The child's row then calls `RemoveNode(child, group, true)`: `RemoveFromParent` finds nothing to do, but `parent.IsLeaf && parent.Element == null` is now true of the dead group, so a brand-new `Element` is created for it and registered with `VixenSystem.Elements` (NodeManager.cs:84-92). That element belongs to no live node, is written to the system config on OK (VixenSystem.cs:185) and survives every later load. **Mac:** `delete_rows` skips any row whose parent link has already gone, and creates nothing. The loader must tolerate `channels` that no node refers to, since the owner's existing show may contain some from this bug; keep them, as upstream does, so ids stay stable.

- [ ] **F45. Create Group.** `CreateGroupFromSelectedNodes` (ElementTree.cs:855-871). Remembers the selection, prompts with `AddSingleNodeWithPrompt()` (no parent, so the group is a new **root** node; name prompt "Element Name?", empty gives "New Element"), adds each original node to it with `AddChildToParent` (a link: the nodes also stay where they were, no removal from old parents), repopulates selecting the new group, raises ElementsChanged when the group was created (:1409-1414). Declining the prompt does nothing.

- [ ] **F46. Rename (context).** Same as F49; raises ElementsChanged, not Rename, from this route (:1416-1422).

- [ ] **F47. Find/Replace.** `PatternRenameSelectedGroup` (ElementTree.cs:896-921). `SubstitutionRenamer(oldNames)` dialog over the display-order selection names. On OK renames rows by index, stopping (with a logged warning) if the dialog returned fewer names, then repopulates. Does **not** raise ElementsChanged (:1424-1427), so the patching view is not told. The Graphical view's titles would stay stale even if it were told (F113). The port refreshes them.

- [ ] **F48. Collapse All.** `treeview.CollapseAll()` (ElementTree.cs:1429-1432).

- [ ] **F49. Rename rules** (`RenameSelectedElements`, ElementTree.cs:923-967). One row: `TextDialog("Item name?", "Rename item", currentName, true)`; on OK with a non-empty changed response `Nodes.RenameNode(node, response)`, repopulate, return true. Unchanged or empty returns false. Several rows: `NameGenerator(oldNames)`; on OK rename by index with the same guard, repopulate, return true. Renaming goes through `RenameNode`, which uniquifies; see the Data section. Rows are renamed one after another in display order, so a new name still held by a selected row later in the list gets a ` - 2` suffix even though that row is about to change. The port keeps this order and result; only a row's own name stops counting as a clash (F138).

- [ ] **F50. Reverse Elements.** For i = 1..n-1 in display-selection order: `MoveNode(row[i], parent, parent, row[0].Index)` (ElementTree.cs:1434-1446). Since the visible tree is not rebuilt during the loop, `row[0].Index` stays constant, so each later row is inserted in turn at the first row's original position, which reverses the run. Available only when `CanReverseElements` holds (F72). Repopulates, raises ElementsChanged. Undo: Cancel only.

- [ ] **F51. Sort.** Sorts the one selected group's direct children by `OrderBy(n => n.Name)` using .NET's default culture-aware string comparison (not natural numeric order, so "Item 10" sorts before "Item 2"), moving each to its index in turn, then repopulates (ElementTree.cs:1478-1494). Raises no ElementsChanged. Undo: Cancel only.

- [ ] **F52. Export Wire Diagram > Front Wire Diagram / Back Wire Diagram.** Invokes `ExportDiagram(firstSelected, flip)` with `flip=false` for Front and `flip=true` for Back (ElementTree.cs:1570-1578); in Display Setup the delegate is `ElementModeling.ElementsToSvg(node, flip)` (SetupElementsTree.cs:71-77). See F128 to F131.

- [ ] **F53. Export Element Tree.** Save dialog with overwrite prompt, default extension ".v3m" (sic) while the filter is "Vixen 3 Element Nodes (*.v3e)|*.v3e", initial directory `SequenceService.SequenceDirectory`; on OK `await VixenSystem.Nodes.ExportElementNodeProxy(path)` (ElementTree.cs:1453-1467). Tooltip "Export the element tree for importing into other profiles." (Designer :227). Exports the whole tree, whatever is selected. The filter wins the extension on Windows, so write `.v3e`.

- [ ] **F54. Tags submenu.** Rebuilt on each menu open (ElementTree.cs:1008-1043), in this order: a label "Ctrl+Click to Cascade Tag", a separator, one item per `ElementTagService.Instance.GetAll()` (service order) with a tri-state check, a separator, "Manage Tags...". Check state over the selection: none tagged unchecked, all tagged checked, some tagged indeterminate (:1067-1079). Items with a `DisplayColor` paint a colour dot right-aligned, inset 6pt from the item's edge (:1059-1065). On the Mac the label reads "⌘-Click to Cascade Tag" and the modifier is ⌘, read from the current event when the item fires.

- [ ] **F55. Toggle a tag.** Click item (ElementTree.cs:1089-1117): the target set is the selected nodes, or with Ctrl held, every selected node and all its descendants, de-duplicated. If the item was fully checked, remove the tag from all targets and uncheck; if unchecked **or** indeterminate, add it to all targets and check. Repaints the tree. Tags are stored on the `ElementNode`. No ElementsChanged is raised. Saved with the system config on OK, discarded on Cancel. A Mac menu closes on click, so the visual check update only shows on next open, which is acceptable.

- [ ] **F56. Manage Tags...** Raises `ManageTagsRequested`; in Display Setup this opens `ElementTagManagerWindow.ShowAsDialog(saveOnClose: false)` and, if it returns true, repaints the dots (SetupElementsTree.cs:370-376). See F132.

- [ ] **F57. Drag and drop in the element tree: moving.** Drag selected rows onto or between rows. Target resolution (ElementTree.cs:566-598): drop **on** a row makes it the new parent, appended (`index = -1`), expanded afterward. Drop **below** an **expanded** row makes that row the parent at index 0. Otherwise the new parent is the target's parent (null for root) at `target.Index` (above) or `target.Index + 1` (below). Prompt F43 for the new parent first. Then per source row: `MoveNode(source, newParent, oldParent[, index])`, where oldParent is that row's parent. With an index: increment index after each move when the parents differ, or when the parent is the same and `index < currentIndex`; when moving later in the same parent, index stays put because the shuffle compensates (:613-630). Then expand the target if any, `PopulateNodeTree()`, `OnDragFinished()` (:649-653). **Display Setup does not listen for DragFinished**, so drags raise no ElementsChanged and no patching view update beyond what selection events cause (SetupElementsTree.Designer.cs:346-349). So when a drop turns a patched leaf into a group, the controller tree keeps showing its old patch status until something else refreshes it. The Mac version refreshes patch status and the patching view after every drop. Undo: Cancel only.

- [ ] **F58. Drag and drop in the element tree: copying (linking).** Ctrl held at drop gives copy: `AddChildToParent(source, newParent[, index++])` (ElementTree.cs:631-642, :696-701), incrementing so order is preserved. "Copy" means the node gains another parent, as with Paste; it is not a duplicate. On the Mac: ⌥ (Option) drag is the system convention for copy, with the green plus cursor; the Mac version uses ⌥.

- [ ] **F59. Drag validity.** Checked continuously on drag over (ElementTree.cs:656-719). Sources are all dragged nodes. Invalid and permitted sets: for a drop on a row, or below an expanded row: invalid = `target.InvalidChildren()`, permitted = empty. Otherwise, with no parent: invalid = `Nodes.InvalidRootNodes`, permitted = root nodes; with a parent: invalid = `parent.InvalidChildren()`, permitted = parent's children. Copy mode sets permitted to empty. If any source is invalid, the drop is valid only when every invalid source is also in permitted (so reordering inside the same parent is allowed but adding a second link to the same parent is not). `treeview_DragStart` is cancelled when `AllowDragging` is false (:554-557). See `InvalidChildren` in the Data section for what counts.

- [ ] **F60. Selection after Paste, Sort, Reverse.** Repopulate preserves the selected paths where those still exist (ElementTree.cs:285-296, :159-162), otherwise drops them. The Add flows change selection to the new rows (`AddNodePathToTree`, :813-853).

### D. Simple Patching view (centre column, default)

Layout (SetupPatchingSimple.Designer.cs:693-711): two group boxes side by side, "Selected Elements" (left) and "Selected Controllers" (right), then "Patching Options" spanning both. The panel scrolls if it does not fit (`AutoScroll = true`, :717). Everything here is read on the element and controller selection as pushed in by F4. Patch-point counting is defined in F61.

- [ ] **F61. Patch points: the precise definition.** A *patch point* is a (component, output index) pair at the end of an element's data flow chain, and `Patched` says whether it goes to a controller. `_findPatchedAndUnpatchedOutputsFromComponent(component)` (SetupPatchingSimple.cs:317-356): null component or null `Outputs` gives an empty list. For each output index i of the component, take `DataFlow.GetDestinationsOfComponentOutput(component, i)`:
  - No destinations: this output is a patch point, `Patched = false`.
  - Some destinations: recurse into each child and gather their results. If at least one child yielded something, the gathered list is the result and this output itself adds nothing. If **no** child yielded anything (every child has no outputs, which in practice means they are controller outputs), this output is one patch point with `Patched = true`.
  So: an element with no filters has `Outputs.Length` patch points of its own; a colour breakdown filter hanging off it replaces that with one per filter output; a patch point is "connected" exactly when it reaches a component with no outputs.

- [ ] **F62. "Selected Elements" counts.** `_UpdateElementDetails` (SetupPatchingSimple.cs:121-143) with `_countTypesDescendingFromElements` (:249-295), which uses `_countedNodes`, a visited set of nodes, so a node reached twice is counted once. Rows, with the bold ones marked (SetupPatchingSimple.cs:25-28):

  | Row label (bold if marked) | Value | Tooltip |
  |---|---|---|
  | **Total Patch Points:** | patched + unpatched | "The total number of Patch Points connected to the selected elements.  Patch Points are the outputs from any element or filter, but before it gets to the controllers." |
  | Connected: | patched | "The number of Patch Points connected to the selected elements that are being patched to controller outputs." |
  | Unconnected: | unpatched | "The number of Patch Points connected to the selected elements that have nothing else connected." |
  | (20pt gap) | | |
  | Items: | number of selected rows' nodes, `nodes.Count`, before any de-dupe | "The number of items selected in the element view." |
  | Groups: | visited nodes with children | "The number of groups found from the selected elements." |
  | Elements: | visited nodes without children (leaves) | "The number of elements found in (or descending from) the selected elements." |
  | Filters: | see below | "The number of filters found in the patching connections from the selected elements." |

  Traversal per node, depth-first (:257-292): skip if visited; recurse over `Children`; then add 1 to groups or leaves; then filters += the count of output filters in the data flow tree under this node's element (`_findFiltersThatDescendFromElement`, :297-306), which walks `GetDestinationsOfComponent` recursively and returns every component assignable to `IOutputFilterModuleInstance` (:308-315). Each component has one source, so a filter is only reached once; finally, if the node has an element, add its data flow patch points (F61). Note the patch points list is built from `node.Element != null` nodes; group nodes normally have no element (see ElementNode).

- [ ] **F63. Unpatch Elements.** Button "Unpatch Elements", centred under the counts (Designer :175). Enabled when connected > 0 (SetupPatchingSimple.cs:142). Flow (:550-625):
  1. If patched count > 20: "Are you sure you want to unpatch {n} patch points?", title "Unpatch Elements?", YES/No, question. No aborts. (At 20 or fewer there is no prompt. Upstream tests only for No, so closing this box with its close button carries on as Yes, here and in F66; the Mac alert has no close button.)
  2. For every leaf of each selected node with an element: take `GetDestinationsOfComponent(leafComponent)` as direct children; for each, add the whole tree under it to a "non-direct" list via `_findComponentsOfTypeInTreeFromComponent(child, IDataFlowComponent)`, which includes the child itself.
  3. If any non-direct component is an output filter: "Some elements are patched to filters.  Should these filters be removed as well?", title "Remove Filters?", YES / No / Cancel, question. Cancel aborts. `removeFilters = Yes`.
  4. For each direct child: `ResetComponentSource(child)` (cut it from the element). If removeFilters and it is a filter: `Filters.RemoveFilter(filter)`.
  5. If removeFilters: remove every filter in the non-direct list too.
  6. Raise PatchingUpdated (refreshes both trees), recompute this view.
  **Quirk: with removeFilters = No**, the filters stay connected to their controller outputs but have no source, so those outputs still show as patched although no element drives them. Then OK's orphan clean-up (F6) removes those filters anyway, and so unpatches those outputs. So "No" only postpones the removal. Keep this. Undo: Cancel only.

- [ ] **F64. Patching order option: "Reverse Element Order".** Checkbox inside the element options box (Designer :313). Tooltip "The order in which Elements will be patched to controller outputs. This does not effect the order in which color channels are patched to controller outputs." Default off. Only reads when patching runs, and has no effect on the summary (SetupPatchingSimple.cs:219-222, :675-678).

- [ ] **F65. "Selected Controllers" counts.** `_updateControllerDetails` (SetupPatchingSimple.cs:363-430). Iterates the selection set in pane order, outputs ascending within each controller. An output index beyond the controller's length logs a warning and is skipped. An output is patched when `Outputs[i].Source != null && Source.Component != null`.

  | Row label | Value | Tooltip |
  |---|---|---|
  | **Outputs:** | patched + unpatched | "The total number of controller outputs selected." |
  | Patched: | | "The number of controller outputs selected that are already connected to something." |
  | Unpatched: | | "The number of controller outputs selected that are not connected to anything." |
  | (20pt gap) | | |
  | Controllers: | controllers in the set | "The number of controllers (or part thereof) selected." |
  | (20pt gap) | | |
  | First output: | `"{Controller.Name} #{outputIndex+1}"`, empty when nothing selected | "The first output in the list of selected outputs (as will be used for patching)." |
  | Last output: | same format | "The last output in the list of selected outputs (as will be used for patching)." |

  The First/Last labels wrap (MaximumSize tracks the column width on resize, :36-42). They are captured **before** the reverse step, so ticking "Reverse order" does not swap them (:399-406), in spite of the tooltip. Keep this.

- [ ] **F66. Unpatch Controllers.** Button "Unpatch Controllers" (Designer :390). Enabled when patched > 0 (SetupPatchingSimple.cs:415). If patched > 20: "Are you sure you want to unpatch {n} patch points?", title "Unpatch Controllers?", YES/No (:634-643; the count is parsed back from the label text, in the port use the model). For every selected output: `ResetComponentSource(outputComponent)` (:645-659). Raise PatchingUpdated, recompute. Filters upstream of those outputs stay connected to their elements, and their outputs become unconnected patch points. Undo: Cancel only.

- [ ] **F67. "Patching Options" group.** (SetupPatchingSimple.Designer.cs:537-616). Left box: radios "Use unconnected patch points only" (default, :595) and "Use all available patch points", then the Reverse Element Order checkbox. Right box: radios "Only use unpatched outputs" (default, :663) and "Use all selected controller outputs", then checkbox "Reverse order of selected outputs" (default off). Below: summary line, warning line (yellow, `#FFFF00`, SetupPatchingSimple.cs:30), then the "Patch Elements" button (12pt, SetupPatchingSimple.cs:29; centred, Designer :608-616). Radios re-run the summary (SetupPatchingSimple.cs:544-547); the output reverse checkbox re-runs the controller details, which reverses the destination list (:665-668). The output reverse checkbox has no tooltip; only the element one does (F64).

- [ ] **F68. Summary and warnings** (`_updatePatchingSummary`, SetupPatchingSimple.cs:438-497). Sources = all patch points, or only the unpatched ones, by radio. Destinations likewise. Text: `"This will patch {sources} element patch points to {destinations} controller outputs."` Warning when both are nonzero: sources > destinations "WARNING: too many elements, some will not be patched"; destinations > sources "WARNING: too many outputs, some will not be patched"; else empty. "Patch Elements" is enabled when both > 0. With nothing selected, which is how the view opens (SetupPatchingSimple.cs:31), the text reads "This will patch 0 element patch points to 0 controller outputs.", every count is 0, First and Last output are blank and all three buttons are disabled. The Designer's placeholder strings ("9999", "Controller XXXX", the warning text) are never shown.

- [ ] **F69. Patch Elements: the exact algorithm** (`buttonDoPatching_Click`, SetupPatchingSimple.cs:499-542; `GetOrderedElementOutputs`, :145-235):
  1. **Leaves.** For each node in `_cachedElementNodes` (the selection in selection order) append `node.GetLeafEnumerator()`, then `Distinct()` keeping first occurrences.
  2. **Preferred order.** Order property id is `OrderDescriptor.ModuleId`. If no leaf has it: tree order stands. If some but not all have it: prompt "Not all elements have a preferred patching order set. Would you like to fix this before proceding?\n\nYes) Open the order wizard.\nNo) Proceed using the element tree order.\nCancel) Abort patching." (sic "proceding"), title "Patching Order Conflict", YES/No/Cancel, warning icon. Cancel aborts. Yes runs the Order setup helper on the selection; if it returns false, abort, else sort. No keeps tree order. If all have it: check duplicates of `Order`. Duplicates: prompt "Some elements have a duplicate preferred patching order set. Would you like to fix this before proceeding?\n\n…" with the same three lines; same handling. No duplicates: `OrderBy(Order)`, a stable sort.
  3. **Reverse.** If Reverse Element Order is ticked, reverse the whole leaf list.
  4. **Sources.** For each leaf with an element, in order, append the patch points of its data flow component (F61) in component output order. This is why colour order within an element is untouched by reverse. Sources are then filtered by the radio (`Patched == false`, or all).
  5. **Destinations.** Recompute controller inputs (`skipStats = true`): pane order, ascending outputs, reversed in total if "Reverse order of selected outputs" is ticked (:376, :403-406). Filtered by radio.
  6. **Connect.** `max = min(sources, destinations)`. For i in 0..max: `DataFlow.SetComponentSource(destination[i], source[i])` (:530-535). Connecting to an already patched output replaces its source. Connecting from an already connected patch point (with "Use all available patch points") adds a second destination to that point: its old output stays patched as well. Extra sources or destinations are left alone. The return value is ignored; a cycle cannot arise here because controller outputs have no outputs of their own.
  7. Raise PatchingUpdated, recompute everything (:537-538).
  8. Show "Patched {max} element patch points to controllers.", title "Patching Complete", OK, information (:540-541). Counts (F62) walk elements on any node while patching (step 1) only takes leaves. They agree because a node gaining children loses its element (`AddChildToParent`).
  Undo: Cancel only. Selection stays, and because the destinations list is recomputed with "unpatched only", pressing the button again patches the next unpatched items.

- [ ] **F70. View state lifetime.** Radios and checkboxes in this view keep their values while the window is open, including while the Graphical view is showing, and reset to defaults at each new window. Not persisted.

### E. Multi-select tree behaviour (`MultiSelectTreeview`, used by both trees)

- [ ] **F71. Mouse selection** (MultiSelectTreeview.cs:327-398, 962-990).
  - Plain click on an unselected row selects only it (mouse down). Plain click on a row already selected in a multi-selection does **nothing**: it keeps the multi-selection, because mouse down defers to mouse up "for a potential drag" and mouse up's condition (:386) only re-selects when the row is *not* selected.
  - Ctrl-click on an unselected row adds it on mouse down, and it becomes the new anchor. Ctrl-click on a selected row removes it, on mouse up, unless the drag started.
  - Shift-click selects the visible rows from the anchor to the clicked row, replacing the selection (:975-979, :1043-1066). The anchor is the last Ctrl-added or plain-selected row, kept across Shift gestures. `_rangeSelectionOrder` records the rows from anchor to endpoint, so a range made upward has reversed selection order. This is what makes "shift-select upward, then patch" patch in reverse (F69 step 1). Any later toggle or clear discards the order (:994, :1029).
  - Selection order otherwise is display order, kept sorted by `TreeNodeSorter` (:252-259). The primary row (`m_SelectedNode`, which the Properties list and Paste use) is the row last acted on, including a row just Ctrl-clicked off (:1027-1041).
  - Right-click: mouse down selects an unselected row (plain click rules) so the context menu applies to it; a selected row keeps the multi-selection (:345-353).
  - A click on empty space (no row) clears the selection and raises Deselected (:363-367). A click on a row but left of its text (including the icon and the expand glyph) or more than 10pt right of its text also clears, when no modifier is held (:337-361), so opening or closing a group with its glyph upstream drops the selection. The upstream comment calls this a workaround for TreeView's full-width hit test.
  - Shift-click with nothing selected selects only the clicked row and makes it the anchor (:967-973).
  - A drag only starts if the initial click landed on the text (:403-404).
  - On the Mac: ⌘ replaces Ctrl and ⇧ stays. Use standard `NSOutlineView` full-row hit testing, so a click anywhere on a row selects it, a click on the disclosure triangle leaves the selection alone, and a click in empty space below the rows clears. Standard `NSOutlineView` also reduces a multi-selection to the clicked row on mouse up when no drag occurred. That is a deliberate improvement on upstream and is recorded in the parity file. Keep anchor-to-endpoint selection order: `NSOutlineView` does not expose it, so track the anchor in the wrapper (⇧-click and ⇧-arrow).

- [ ] **F72. `CanReverseElements`** (MultiSelectTreeview.cs:263-295). True when the selected rows all share one parent and their indexes are consecutive in display order. Gates Reverse Elements (F50).

- [ ] **F73. Keyboard navigation** (`ProcessKeyboardSelection`, MultiSelectTreeview.cs:467-604). Applies only with no modifier, or Shift-only with Up, Down, Home or End.
  - No current row: selects the top visible row first (Shift+Down then stops there).
  - Left: collapse if expanded with children, else select the parent. Right: expand if collapsed, else select the first child.
  - Up / Down: previous / next visible row. Shift extends the range from the anchor.
  - Home: first root row (Shift: range to it). End: last visible row, not expanding anything (Shift: range to it).
  - Page Up / Page Down: moves `VisibleCount` visible rows and selects that one row.
  - A-Z, 0-9: type-select. Finds the next visible row *after* the current one whose text starts with the character, no wrap. Because the key code gives an upper case character and the comparison is case-sensitive, lower-case names are never matched (:580-593).
  - Upstream has no Select All in either tree.
  - On the Mac `NSOutlineView` provides arrows, Home/End, page keys and type-select natively (⌥-arrows to expand recursively), and ⌘A selects all visible rows, which is harmless and stays. Leave type-select to the system, which fixes the case quirk.

- [ ] **F74. Drag visuals and hit zones** (MultiSelectTreeview.cs:638-738). Over a row: within 3pt of its top edge means "above", within 3pt of its bottom edge "below", otherwise "on". Between-row drops draw a 2pt dotted line from the row's text left to 10pt right of its text. "On" highlights the row. A target among the dragged rows is invalid and shows no-drop. Dragged rows are drawn in the source colour during the drag. Pointer within 10pt of the top or bottom of the view scrolls one line per drag-over event. Esc cancels. The right mouse button starts the same drag as the left, since `OnItemDrag` does not look at the button (:400-422); the unused "Move Here / Copy Here" menu it was meant to show is listed under omissions. The Mac version drags with the primary button only and takes all of this from `NSOutlineView`'s drop feedback, mapping "on" to `NSOutlineViewDropOnItemIndex` and "between" to a child index.

- [ ] **F75. Drop refusals in the control** (MultiSelectTreeview.cs:758-790). Nothing happens when: there is no target, the target is one of the dragged rows, or the target's `FullPath` **starts with** a dragged row's `FullPath`. That last test is on display text, so besides correctly blocking a drop into a row's own descendants it also silently refuses dropping "Tree" next to or onto a sibling named "Tree 2" (the path "Tree 2" starts with "Tree"). The port does a real ancestor test on node paths and does not keep the prefix bug.

- [ ] **F153. Dragging between the two trees.** Upstream does not stop it, because both trees accept any dragged rows (MultiSelectTreeview.cs:320, :687-690). Element rows dropped between controller rows are moved there visually, with no model effect: the row disappears from the element tree and appears among the controllers until the next repopulate (:783-784, :807-846, with no `DragFinishing` handler on this tree), and the list is marked as reordered (F79). Controller rows dropped on the element tree crash: the verify handler casts them to null element nodes (ElementTree.cs:662) and the drop calls `MoveNode(null, …)` (:610, :617). Dragging an element onto an output is not a way to patch in either tree; that is Patch Elements (F69) or the canvas (F119). Read from the code, not seen running. **Mac:** each tree accepts drags only from itself.

### F. Controllers panel (right column)

Layout top to bottom: Add row, controller tree (fills the height), a row with one button, "Selected Controller" group (SetupControllersSimple.Designer.cs:296-314).

- [ ] **F76. Add row.** Label "Add:", drop-down `comboBoxNewControllerType` with every controller module by `TypeName`, sorted alphabetically, first selected by default (SetupControllersSimple.cs:40-49). To its right three buttons: Add ("Add", + icon), "Move Controller(s) up" (arrow up), "Move Controller(s) down" (arrow down) (Designer :202,:215,:228). Add is enabled when a type is selected; both move buttons when at least one controller is selected (SetupControllersSimple.cs:80-81, :87).

- [ ] **F77. Add Controller flow** (`AddNewControllerOfTypeWithPrompts`, ControllerTree.cs:783-821). Prompt 1: `TextDialog("New Controller Name?", "Controller Name", typeName, true)`; Cancel aborts; empty means the type name. Prompt 2: `NumberDialog("Controller Output Count", "Outputs on this controller?", 1, 1, oc.OutputLimit)`, a value from 1 to the module's output limit, default 1; Cancel aborts and the device is discarded. Then set `OutputCount`, `OutputControllers.Add(oc)`, append a root row, select only it, scroll to it, raise ControllerSelectionChanged and ControllersChanged (:174-192), then the panel scrolls the selection to the top (SetupControllersSimple.cs:185-187). Undo: Cancel only. See the Data section for what `Add` does at manager level. **Upstream bug, not to port:** only the selected controller set is reset here, so output rows that were selected before stay in the logical selection (ControllerTree.cs:177-181) and still count in the patching view, and their rows keep their highlight because the list is cleared directly. The Mac selects only the new controller.

- [ ] **F78. Move Controller(s) up / down.** `ReorderSelectedNodes(BACKWARD | FORWARD)` (ControllerTree.cs:1009-1042; SetupControllersSimple.cs:190-197). Only root rows move. Selected roots are removed bottom to top, then reinserted top to bottom, each at original index ±1, except that a node whose target is at or above the previous insertion point goes to its own original index, which keeps a block at the top from reordering. Inserting beyond the end clamps. Expected results: `[A,B,C,D]` with B,C selected and Up gives `[B,C,A,D]`; Down gives `[A,D,B,C]`; A,C with Up gives `[A,C,B,D]`; A,B with Up is unchanged; C,D with Down is unchanged. **The order only reaches the model at OK** (F6, `ReorderControllers`, ControllerTree.cs:1047-1066) and only if `listReordered` is set. **Quirk:** anything that repopulates the tree from the model before OK (Delete, Channel Count, Insert Channels, Remove Channels, Find outputs) throws the visible reorder away, and OK then writes back the old order. The move also raises no event, so while two or more controllers are selected the patching view keeps its destination order and First/Last labels in the old pane order until the selection next changes (ControllerTree.cs:1009-1042 raises nothing, and `SetupPatchingSimple` only rereads the set when told). The port applies each move to the Rust model straight away and recomputes the patching view, so both quirks disappear and Cancel still reverts it. Multi-select stays.

- [ ] **F79. Drag to reorder controllers.** Drag root rows between other rows (ControllerTree.cs:1361-1365; drop "on" a row is refused). The control itself moves the tree rows (no DragFinishing handler is set, so the default code at MultiSelectTreeview.cs:807-853 runs). `listReordered` is set on every drag-over event, on or between rows, so any drag that crosses a row marks the list as reordered even if it is then cancelled (ControllerTree.cs:1361-1365), and OK then runs the full `Reorder` with its device restarts. **Upstream oddity:** output rows can be dragged too, and rows can be dropped between other controllers' outputs, which rearranges only the visible rows with no model effect (and a controller dropped under another is lost from `ReorderControllers`' list). The port allows reordering root rows among root rows only, ignoring all other drags.

- [ ] **F80. Controller tree rows.** Root rows: `controller.Name`, in `VixenSystem.OutputControllers` order. Icon: **"Group" icon when running, red ball when not running** (ControllerTree.cs:605-611). `treeIconsImageList` provides the same seven images as the element tree. Children appear on expand, with a placeholder when `OutputCount > 0` (:363-366):
  - `OutputCount <= 5000` (`OutputPageSize`, :15): one row per output.
  - `OutputCount > 5000`: page rows named `"Outputs {start+1}-{start+count}"` at 5000 per page, with the outputs under them (:388-398). Selecting a page row selects no outputs (F81). Page rows set no image key, so they show the image list's first entry, the Group folder (Designer :56); the Mac shows the folder icon too.
  - Output row text is `output.Name`, icon by `output.Source` (:419-427): null gives white ball; a reference whose `Component` is null or `OutputIndex < 0` gives grey ball; otherwise green ball.
  - Collapsing a controller or page row discards its child rows and restores the placeholder, then re-applies the logical selection to whatever remains (:1278-1283, :1309-1349). Selection and expansion survive repopulates by identity (controller id, output index, page start), not by name (:17-20, :199-239). Not persisted across opens.
  On the Mac, `NSOutlineView` makes eviction unnecessary, but keep the page rows above 5000: they are the visible structure and a navigation aid on large pixel controllers.

- [ ] **F81. Logical controller selection.** Selection is held as two sets: controller ids and (controller id, output index) (ControllerTree.cs:30-34, :735-754). A selected controller row means *all* its outputs, whether or not they are materialised (:694-720). Page rows contribute nothing. `GetSelectedControllerOutputs` walks root rows in display order; a selected controller gives all its outputs (even if individual outputs were also selected), otherwise the selected outputs in range. Each selection change raises ControllerSelectionChanged, re-evaluates the panel buttons and pushes to the patching view (SetupControllersSimple.cs:60-64). Setting a selection from outside (`PopulateControllerTree(set)`) rebuilds, expands each controller and page that holds a selected output, projects the selection, and raises ControllerSelectionChanged (ControllerTree.cs:81-86, :501-538).
  **Hidden outputs stay selected.** Collapsing a controller or page removes its rows from the visible selection but not from the two logical sets (:1309-1349). So the Selected Controllers counts, First and Last labels and the Find button keep including those outputs, and they come back highlighted on re-expansion. Meanwhile the row-based rules see only what is on screen: the context menu is suppressed when no row is highlighted (F93), and Remove Channels only removes highlighted rows (F95). **Mac:** hold the logical selection in the model, not in `NSOutlineView`, which drops hidden rows from its own selection when a parent collapses. Do not read the selection back from the view after a collapse, and re-highlight the rows on expansion.

- [ ] **F82. Double-click.** On any row with at least one controller selected: `ConfigureController(first selected controller)` (ControllerTree.cs:1356-1359). Double-click does not expand or collapse, unlike the Windows default (:1278-1294, :1351-1354). Double-clicking an output row with only outputs selected does nothing.

- [ ] **F83. Keyboard.** Delete key with at least one controller selected: `DeleteControllersWithPrompt` (ControllerTree.cs:1198-1206). No effect when only outputs are selected. Other keys as F73. Mac: Delete and forward delete. No copy or paste.

- [ ] **F84. "Find elements patched to these outputs" button.** Under the tree, tooltip as named (SetupControllersSimple.Designer.cs:270). Enabled when `GetSelectedControllerOutputs().Any()` (SetupControllersSimple.cs:89). Also bound to the context item "Find Patched Element(s)" (:55). Algorithm (:227-266): for each selected output of each selected `OutputController`, follow `Source.Component` upward to the root (:268-278); if the root is an `ElementDataFlowAdapter`, map its element to its node via `VixenSystem.Elements.GetElementNodeForElement`; collect (duplicates allowed). Empty: "No element patch points found.", title "Not Found", information icon. Otherwise `MasterForm.SelectElements(list, true)`: selects them in the element tree and scrolls to the first. No change to data.

- [ ] **F85. "Selected Controller" group: read-outs.** "Type:" and "Outputs:" labels (SetupControllersSimple.cs:91-110). No controller selected (including when only outputs are selected): both blank. One: the module's `TypeName`, and `OutputCount`. Several: `"{n} controllers selected"` and the sum of `OutputCount`.

- [ ] **F86. "Selected Controller" group: buttons,** in order left to right (x positions in SetupControllersSimple.Designer.cs:154-185): Configure (cog, tooltip "Configure"), Channel Count (tooltip "Channel Count"), Rename (pencil, "Rename"), Delete (delete icon, "Delete"), Start ("Start / Enable"), Stop ("Stop / Disable"). Enable rules (SetupControllersSimple.cs:74-87):

  | Control | Enabled when |
  |---|---|
  | Configure, Channel Count, Rename | exactly one controller selected |
  | Delete, Move up, Move down | at least one controller selected |
  | Start / Enable | at least one selected controller is not running |
  | Stop / Disable | at least one selected controller is running |
  | Add | a controller type is selected |
  | Find elements | any selected controller or output |

  The panel re-evaluates these on selection change, on ControllersChanged and after Start or Stop.

- [ ] **F87. Configure Controller.** `ConfigureController(controller)` (ControllerTree.cs:868-880). `HasSetup` false does nothing (the button stays enabled). Otherwise runs the module's setup dialog; on true, raises ControllersChanged and refreshes output names. Per-module dialogs are specified in docs/spec/property-filter-controller-setup.md. Undo: Cancel only.

- [ ] **F88. Channel Count.** `SetControllerOutputCount` (ControllerTree.cs:882-894). `NumberDialog("Controller Output Count", "Outputs on this controller?", currentCount)`, with its default bounds of 0 and no upper limit (F148). On OK sets `OutputCount`, raises ControllersChanged, repopulates. Shrinking drops outputs and their patches with **no warning**; see the Data section for the setter's exact semantics. Undo: Cancel only.

- [ ] **F89. Rename Controller.** `RenameControllerWithPrompt` (ControllerTree.cs:824-837). `TextDialog("Controller Name?", "Controller Name", name, true)`; on OK with a non-empty response sets `Name`, raises ControllersChanged, updates the row. No uniqueness rule here. Undo: Cancel only.

- [ ] **F90. Delete Controller(s).** `DeleteControllersWithPrompt` (ControllerTree.cs:839-866). One: "Are you sure you want to delete the selected controller?", title "Delete Controller?". Several: "Are you sure you want to delete the selected controllers?", title "Delete Controllers?". YES/No, warning icon. On Yes `OutputControllers.Remove(oc)` each, raise ControllersChanged, repopulate. Undo: Cancel only. **Upstream leaves stale state:** ControllersChanged fires while the deleted rows are still in the tree, and the repopulate raises nothing. So the patching view keeps counting the deleted controller, with 0 outputs ("Controllers: 1"), and the "Selected Controller" group keeps its type and "0" and its enabled buttons, until the next selection change (:860-861; SetupControllersSimple.cs:66-70). The Mac clears the selection and refreshes both.

- [ ] **F91. Start / Stop.** Start: for each selected controller not running, `OutputControllers.Start(...)`; Stop: for each running one, `OutputControllers.Stop(...)` (ControllerTree.cs:1208-1240). If anything changed: refresh root icons and raise ControllersChanged. These change hardware state at once. OK persists them through the disabled-devices list, and Cancel's reload goes back to the saved state (F7, Data section). On the Mac the state goes through `ControllerRunner`, and output controllers stay stopped in tests unless a test starts them (docs/UI_GUIDE.md).

### Context menu on the controller tree

Items, in `Items.AddRange` order (ControllerTree.Designer.cs:67-80): Configure, Channel Count, Insert Channels, Remove Channels, Unpatch Channels, Find Patched Elements, Rename, Delete, Start / Enable, Stop / Disable, separator, Unpatch Controllers. Which ones appear depends on the selection (`contextMenuStripTreeView_Opening`, ControllerTree.cs:1074-1150).

- [ ] **F92. Menu when at least one controller is selected** (:1077-1097), even if outputs are also selected: shows Configure, Channel Count, Rename, Delete, Start / Enable, Stop / Disable, separator, Unpatch Controller(s). Hides the four channel items. Configure, Channel Count and Rename are enabled only with exactly one controller; Delete with any. `CheckIfSelectedControllersRunning` (:1242-1276) enables Start when at least one is not running, Stop when at least one is running, and Unpatch when any selected controller has an output whose `Source.Component` is not null. Wording: "Unpatch Controller" for one and "Unpatch Controllers" for several.

- [ ] **F93. Menu when only output rows are selected** (:1099-1147): shows Insert Channels (exactly one output selected), Remove Channels, Unpatch Channel(s), Find Patched Element(s). Hides everything else including the separator. "Unpatch" and "Find" are enabled when at least one selected row shows a green ball. Wording singular with one row ("Unpatch Channel", "Find Patched Element"), plural otherwise. A selection with any non-output row (a page row) suppresses the menu, and so does an empty selection (:1101-1105, :1149).

- [ ] **F94. Insert Channels.** With exactly one output row selected. `NumberDialog("Insert Outputs", "Number of outputs to insert.", 10)`, default bounds (F148). On OK, with a wait cursor on a background task: if running, `OutputControllers.Pause(controller)`; `controller.InsertOutputsAt(outputIndex, n)`; if it was running, `Resume`. Then raise ControllersChanged and repopulate (ControllerTree.cs:896-937). Inserts at the selected output's index, pushing it and later ones down. Undo: Cancel only.

- [ ] **F95. Remove Channels.** Removes every highlighted output row, grouped by controller (ControllerTree.cs:939-998); outputs that are selected but hidden in a collapsed page or controller are not removed (F81). Returns false when any controller is selected, which cannot happen via this menu. For each controller: pause if running, `controller.RemoveOutputs(outputs)`, resume. Clears the highlighted rows, raises ControllersChanged, repopulates. No confirmation prompt. Undo: Cancel only.
  **Selection afterwards:** only the rows' highlight is cleared, not the logical set of (controller, index) pairs, and survivors are re-indexed. So the repopulate re-selects whichever outputs now sit at the removed indexes, and an index past the new end is dropped (:982-984, :501-538). Removing output 3 of 10 therefore leaves the new output 3 selected, and pressing Remove Channels again removes the next one. Keep this.

- [ ] **F96. Unpatch Channel(s) / Unpatch Controller(s).** Both call `DisplaySetup.control_UnpatchControllers` (F5), which runs Simple Patching's Unpatch Controllers click (F66) on the current selection, including the "more than 20" prompt.

- [ ] **F97. Find Patched Element(s).** Same handler as the Find button, F84.

- [ ] **F98. Other context items.** Configure F87, Channel Count F88, Rename F89, Delete F90, Start / Stop F91.

### G. Element templates (the "Add:" list)

- [ ] **F99. Template list.** Upstream finds templates by reflection, so its order is not a contract (ApplicationServices.cs:125-134). All are constructed once when Display Setup opens (DisplaySetup.cs:33) and the same instance stays in the combo, so **each template remembers its last values, including after Cancel, until Display Setup closes** (each form copies its fields in FormClosed, which fires on OK and Cancel alike, for example Megatree.cs:163-179, and `Form.Load` copies them back on the next `ShowDialog`). The Mac version keeps one in-memory draft per template, per window. The Mac list is fixed. Reflection normally follows compile order, which is alphabetical by file, so this is the order the owner is most likely used to (an inference, not something the code states; check it with the owner): "Arch", "Candy Cane", "Icicles", "Intelligent Fixture", "LipSync", "Megatree", "Generic Numbered Group", "Pixel Grid / Matrix", "Single Item" (selected by default), "Star", "Starburst". All templates create their head at the **root** whatever is selected: `selectedNodes` is accepted but none reads it, and every `CreateSingle` passes `parent = null`.

- [ ] **F100. Shared add flow** (`ElementTemplateHelper.ProcessElementTemplate`, Vixen.Common/Utilities/ElementTemplateHelper.cs:27-104):
  1. `SetupTemplate(selected)`: shows the template's dialog. False stops everything.
  2. `await GenerateElements(selected)`: creates the nodes immediately (live edit).
  3. If `template.Cancelled` is true, skip to step 7.
  4. If nothing was created: "Could not create elements.\nPlease close all dialogs and try again.\n\nPlease send error logs to Vixen Team.", title "Error", OK and Cancel buttons, error icon; stop.
  5. If `ConfigureDimming` (true except Intelligent Fixture): ask "Would you like to configure a dimming curve for this Prop?", title "Dimming Curve Setup", YES/No, question icon. Yes: `new DimmingCurveHelper(true).Perform(created)`. That helper is in docs/spec/property-filter-controller-setup.md.
  6. If `ConfigureColor` (true except Intelligent Fixture): `new ColorSetupHelper()`, `SetColorType(FullColor)`, `Perform(created)`, without asking first. `created` includes group nodes as well as leaves; the helpers decide what to touch. **Cancelling either helper does not remove the created elements.**
  7. If cancelled: return nothing. Otherwise call the panel's add callback with `created.First()` only (F10), then `CleanupTree`: repopulate; if `GetElementsToDelete()` is non-empty, for each such node select it, delete its selected row(s), repopulate, and select `created.First().Children.First()` (ElementTemplateHelper.cs:116-146). This same helper also serves Preview setup (Vixen.Modules/Preview/VixenPreview/VixenPreviewSetupElementsDocument.cs:26), so the Rust side keeps it free of any window.

- [ ] **F101. Name uniquifying inside templates.** `NamingUtilities.Uniquify(names, name)` is the same `"{name} - {n}"` rule from 2 up, but against a `HashSet` of existing names taken once at the start of generation (Vixen.Core/Utility/NamingUtilities.cs; for example Megatree.cs:73). The set is never updated during generation, and nodes are then created with `uniquifyName: false`. Children's names are built from the **head's final name** in Megatree, Pixel Grid, Starburst and Icicles (so "Megatree - 2 S1 Px1"), and from the raw prefix in Numbered Group (so "Star Px-1 - 2"). LipSync differs: it uses `CreateSingle`'s own uniquify and builds children from the **typed name**, so a second LipSync is head "LipSync - 2" with children "LipSync Outline - 2", and so on (LipSync.cs:48-55). Keep these name results exactly, because sequences reference by id but users see these names and existing shows contain them.

- [ ] **F102. Single Item** (ElementTemplates/SingleItem.cs). Default template. `TextDialog("New Element Name?", "Element Name", lastName, true)`, initial "New Item", with the text selected. Empty response becomes "New Item". Creates one root leaf `CreateSingle(null, name)`. Then F100 steps 5 and 6.

- [ ] **F103. Generic Numbered Group, Arch, Candy Cane, Star** (NumberedGroup.cs, Arch.cs, CandyCane.cs, Star.cs). One dialog, title "Group Setup" for all (Designer :178), minimum size 360 x 221. Fields: "Group Name:" text, "Item Prefix:" text, "Item Count:" number 1 to 1000 (Designer :119-120). OK, Cancel.

  | Template | TemplateName | Group default | Prefix default | Count default |
  |---|---|---|---|---|
  | NumberedGroup | Generic Numbered Group | Group | Item | 10 |
  | Arch | Arch | Arch | Arch Px | 25 |
  | CandyCane | Candy Cane | CandyCane | CandyCane Px | 25 |
  | Star | Star | Star | Star Px | 40 |

  Generation (NumberedGroup.cs:52-88): empty group name or empty prefix logs an error and returns nothing, which triggers the "Could not create elements" message. Head = uniquified group name at root. Children = `"{prefix}-{i+1}"` uniquified, in order, each a leaf. No Order property. Then F100 steps 5 and 6.

- [ ] **F104. Starburst** (StarBurst.cs). Title "Starburst Setup" (class name `Starburst`). Fields (StarBurst.Designer.cs:50-210): "Starburst Name:" default "Starburst"; "Spoke Count:" 1 to 1000, default 8; "Spoke Prefix:" default "S"; "Pixel Starburst:" checkbox, default off (`_pixelTree = false` at StarBurst.cs:26 overrides the Designer's checked state at Load, :104); "Pixels / Spoke:" 1 to 1000, default 50, and "Pixel Prefix:" default "Px", both enabled only when Pixel Starburst is on (:94-98). Generation (:45-92): empty name returns nothing. Head uniquified at root. Each spoke `"{head} {spokePrefix}{i+1}"`. With pixels, each pixel `"{spoke} {pixelPrefix}{j+1}"`. No Order property. Default run gives "Starburst" with "Starburst S1".."Starburst S8".

- [ ] **F105. Icicles** (Icicles.cs). Title "Icicles Setup", fixed size (Designer :204-208). Fields: "Icicle Name:" default "Icicles"; "String Count:" 1 to 1000, default 50; "Pixels / String Pattern:" text default "7,9,5", with the hint "Example: 7,9,5" below; "String Prefix:" default "S"; "Pixel Prefix:" default "Px". The pattern box only accepts digits, commas and control keys (Icicles.cs:138-145); on the Mac filter the same way, and also handle paste, which upstream does not filter (paste can put non-digits through, making `Convert.ToInt16` throw at :88; the port validates instead). On OK with no number in the pattern (split on "," removing empty entries): "Please enter at least one pixel count in the String Pattern field, e.g. 7,9,5.", title "Invalid String Pattern", warning; the dialog stays open (:115-129). Cancel skips validation. Generation (:44-106): empty name returns nothing. Head uniquified at root. Cycling through the pattern (`i % len == 0` resets, else advance), each string `"{head} {stringPrefix}{i+1}"` gets `pattern[k]` pixels `"{string} {pixelPrefix}{j+1}"`. Values are parsed as 16-bit: over 32767 throws upstream, and the port rejects them with the same message. A zero in the pattern gives a string that is a leaf. No Order property. Expected with defaults: 50 strings with 7, 9, 5, 7, 9, 5, ... pixels, that is 16 full cycles (48 strings, 336 pixels) plus strings of 7 and 9, so 352 leaves.

- [ ] **F106. LipSync** (LipSync.cs). Title "LipSync Setup". One field "Element Name:" default "LipSync". Generation (:38-58): empty name returns nothing. Head at root, then eight children named `"{name} {suffix}"` for suffixes in this order: Outline, Eyes Open, Eyes Closed, Mouth Top, Mouth Middle, Mouth Bottom, Mouth Narrow, Mouth O (:12). No Order property. The template only creates the nodes.

- [ ] **F107. Megatree** (Megatree.cs). Title "Megatree Setup", fixed dialog (Designer :309, :317). Fields, top to bottom (Designer y positions): "Tree Name:" default "Megatree"; "String Count:" 1 to 1000, default 16, and "String Prefix:" default "S" on the same row; "Pixel Tree:" checkbox, default off (`_pixelTree = false`, :31); "Pixels / String:" 1 to 1000, default 50, and "Pixel Prefix:" default "Px" on one row; group "Wire Start Location" with radios "Bottom Left" (default) and "Bottom Right"; group "Patching " with checkbox "Zig Zag Every" followed by a read-only number. **Pixels / String, Pixel Prefix, both groups, the checkbox and the number are all disabled unless Pixel Tree is on** (:134-142). The number shows Pixels / String when Zig Zag is checked, 0 otherwise, updating live (:186-198). Generation (:50-132): empty name, or a negative count, returns nothing. Head uniquified at root; each string `"{head} {stringPrefix}{i+1}"`; with Pixel Tree each pixel `"{string} {pixelPrefix}{j+1}"`.

  Order property (`Order` is `OrderDescriptor.ModuleId`), written to leaves:

  | Wire start | Zig Zag off | Zig Zag on |
  |---|---|---|
  | Bottom Left | no Order property at all | all leaves in tree order, `AddPatchingOrder(leaves, every)` |
  | Bottom Right | `AddPatchingOrder(strings in order, each string's leaves reversed)` | same list with `every` |

  The Order logic does not look at Pixel Tree. If Bottom Right or Zig Zag was set and Pixel Tree then unticked, the disabled controls keep their values and still apply, so each string, being a leaf, gets an Order property. Also present in code but unreachable from this dialog (only the two radios exist, and FormClosed maps anything else to Bottom Left, :171-178): Top Left is strings reversed, leaves in order; Top Right is all leaves reversed (:113-120). The Rust function should take all four, matching `StartLocation` (StartLocation.cs).

- [ ] **F108. Pixel Grid / Matrix** (PixelGrid.cs). Title "Pixel Grid / Matrix Setup". Fields: "Name" default "Grid 1"; group "String Orientation" radios "Horizontal" (default), "Vertical"; group "Dimensions" "Height" 1 to 1000 default 20, "Width" 1 to 1000 default 32; "String Prefix:" default "R", "Pixel Prefix:" default "C"; group "Wire Start Location" radios Top Left, Top Right, Bottom Left (default), Bottom Right, laid out as a 2 by 2 matching those corners; group "Patching " with "Zig Zag Every" and the read-only number. Changing orientation **swaps the text of the two prefix boxes** (:242-248), so Vertical shows "C" and "R". The number shows Width when Horizontal, Height when Vertical, 0 if unchecked, updated live (:255-272). Generation (:56-184): empty name returns nothing. Head uniquified at root. When Horizontal: the head gets an Orientation property (add it if absent) set to Horizontal (:84-101); Vertical leaves no Orientation property. Outer loop runs `rows` times when Horizontal, `columns` when Vertical; inner loop the other. Names `"{head} {firstPrefix}{i+1}"` and `"{first} {secondPrefix}{j+1}"`. Default result: "Grid 1", "Grid 1 R1", "Grid 1 R1 C1".

  Order property on leaves (`S` = the head's children, each child's leaves `L(s)`):

  | Start | Horizontal | Vertical |
  |---|---|---|
  | Bottom Left | no Order unless Zig Zag: all leaves in tree order with `every` | same |
  | Bottom Right | `S` in order, each `L(s)` reversed (see below) | `S` reversed, `L(s)` in order |
  | Top Left | `S` reversed, `L(s)` in order | `S` in order, each `L(s)` reversed |
  | Top Right | all leaves reversed | all leaves reversed |

  **Upstream bug, fixed in the port:** upstream numbers horizontal Bottom Right as all leaves reversed (:151), the same as Top Right, so the wire starts at the top-right corner. Every other corner puts R1 at the bottom and C1 on the left, so the Mac starts Bottom Right at R1's right end: each row reversed, rows in order.

  For every row but the Bottom Left one: Zig Zag off gives `AddPatchingOrder(list)`, on gives `AddPatchingOrder(list, every)`.

- [ ] **F109. `AddPatchingOrder` exact numbering** (Vixen.Modules/Property/Order/OrderModule.cs:53-95). Both forms add an Order property (if the node has none, default order 0) then set its value. Plain form: 1, 2, 3, ... in list order. Zig zag form with `every = n`: the list is grouped in runs of n; odd runs count up from their first index, even runs count down from their last. For n = 3 and nine nodes: 1, 2, 3, 6, 5, 4, 7, 8, 9. Since `Properties.Add` returns null when the property already exists, an existing Order property is not overwritten. That cannot happen with these freshly created leaves. `every = 0` flips after every node, but cannot occur because the number is at least 1 whenever Zig Zag is checked.

- [ ] **F110. Intelligent Fixture.** `SetupTemplate` does nothing (returns true); `GenerateElements` opens the wizard (section L). `ConfigureColor` and `ConfigureDimming` are both false, so F100 steps 5 and 6 do not run (IntelligentFixtureTemplate.cs:1327-1346). `Cancelled = result.HasValue && !result.Value` from the wizard's dialog result (:1242-1245).

  On finish (:1251-1310): save the fixture profile via `FixtureSpecificationManager.Instance().Save(profile)`. For index in 0..NumberOfFixtures create a fixture node named `ElementPrefix + (index+1)` (uniquified by `CreateSingle`, no separating space), as below. If "create group": add a group node at root with `CreateSingle(null, GroupName, createElement: false)`, add all fixtures as its children, record the fixtures as "to delete" so that `CleanupTree` removes their root rows (they stay under the group), and return only the group; else return the fixtures. After a group run the tree ends with the group expanded and its first fixture selected (F100 step 7), raised without a selection event. Without a group only the first fixture's row is added by the callback, but `CleanupTree`'s repopulate shows them all. The group's name goes through `CreateSingle`'s own uniquify. **Upstream bug, not to port:** `_nodesToDelete` is never cleared between uses and the template instance is reused (F99), so a later run without a group would delete rows of the earlier group's fixtures and then throw on `Children.First()`. In Rust this list is part of one generation's result.

  Fixture node creation (`CreateFixtureNode`, :1072-1165):
  1. Create a root leaf, add the Intelligent Fixture property, assign the fixture specification.
  2. If `colorMixing`: add a Color property with `ColorType = FullColor` (:192-202).
  3. For each channel in `ChannelDefinitions` in order, find its function by name (`Single`; a missing function throws, and the wizard prevents that case). Process the function only when the channel's function name differs from the previous processed channel's, **or** it is the "None" function's description (so several "None" channels each get a filter, while a run of channels sharing one function yields one filter) (:1137-1158).

  Function dispatch (`ProcessFunction`, :1015-1046):

  | FunctionType | Action |
  |---|---|
  | ColorWheel, auto control on | Color wheel filter: `ColorWheelData`, `ConvertRGBIntoIndexCommands = !colorMixing`, `Tag = function.Name`, `CreateOutput`, connect to element output 0. If no colour property exists on this node yet, add a discrete one: `MultipleDiscreteColors`, a colour set named after the fixture holding every `Color1` of every colour wheel function entry that is not `HalfStep`, not `UseCurve`, not `ExcludeColorProperty`, stored by `ColorStaticData.SetColorSet(fixture.Name, set)` with `ColorSetName = fixture.Name` (:156-186, :209-286) |
  | ColorWheel, auto control off | as Indexed |
  | RGBWColor | Color mixing with Red, Green, Blue, White (colours `Color.Red`, `Color.Lime`, `Color.Blue`, `Color.White`), 4 expected colours |
  | RGBColor | Color mixing with Red, Green, Blue, 3 expected colours |
  | Indexed | `FunctionIdentity == Shutter` and auto shutter: Shutter filter. Else `FunctionIdentity == OpenClosePrism`: Prism filter (**the auto prism option is passed but never read**, :964-975, so this happens either way). Else Tagged filter, with no dimming curve |
  | Range | `FunctionIdentity == Dim` and auto dimmer: Dimming filter. Else Tagged filter |
  | None | Tagged filter with `Tag = "Ignore"`, so no intents match (:814-834) |

  After the switch: if this function and the next channel's function are both Range and have the same name, add a Coarse/Fine breakdown filter under the current last leaf output (:1049-1056, :887-898). This is how a 16-bit range gets its two outputs.

  Filter details:
  - **Tagged** (:845-881): `Tag = function.Name`, `CreateOutput`, source = element output 0.
  - **Dimming** (:637-679): `Tag = function.Name`, `ConvertColorIntoDimmingIntents = !colorMixing`, `CreateOutput`, source = element output 0. If a curve selection other than none **and** not colour mixing: add a Dimming Curve filter (copy of the curve) under the last leaf.
  - **Shutter** (:752-808): first `IndexData` entry of type `ShutterOpen` with a nonzero start gives `OpenShutterIndexValue`; likewise `ShutterClosed` gives `CloseShutterIndexValue`. `Tag`, `ConvertColorIntoShutterIntents = true`, `CreateOutput`, source = element output 0.
  - **Prism** (:686-744): same pattern from `PrismOpen`, `PrismClose` into `OpenPrismIndexValue`, `ClosePrismIndexValue`, plus `Tag`, `ConvertPrismIntentsIntoOpenPrismIntents = true`, `AssociatedFunctionName = function.AssociatedFunctionName`, `CreateOutput`, source = element output 0.
  - **Color mixing** (`ProcessColorMixingFunction`, :390-537) with `C` = count of channels on the fixture whose function type is RGB or RGBW (:353-378). First colour channel only:
    1. `OneDimmingCurvePerFixture`: add a Dimming Curve filter on element output 0 first.
    2. Colour Breakdown filter with the items above, `MixColors = true`, `_16Bit = (C > 4)`.
    3. Source: the last leaf output if per-fixture curve is in place, else element output 0.
    4. `OneDimmingCurvePerColor`: for each colour output i of the breakdown, add a Dimming Curve filter with source (breakdown, i). If also 16-bit, for each colour, add a Coarse/Fine filter under the dimming curve found by walking leaves from the end (`updated[count - i - 1]`, :483-499). That walk is `FindLeafOutputsOrBreakdownFilters`.
    5. Otherwise, if 16-bit: for each colour output i add a Coarse/Fine filter with source (breakdown, i).
    Later colour channels only count up; when the count reaches `C`, the "found" state resets (:524-536).
  - `FindLeafOutputsOrBreakdownFilters` (:1172-1209): walks depth first. A `ColorBreakdownModule` yields a reference with output index -1 first, and then also its leaf outputs; so "the last entry" after adding a breakdown is the breakdown's last output (its `Component` is the breakdown). A component with null outputs or `OutputDataType.None` stops the walk.

  Because every function's filter hangs off element output 0 in channel order, the leaf patch points come out in **DMX channel order**, which is what makes Simple Patching wire a fixture correctly in one click.

### H. Graphical View (centre column, alternative)

Upstream draws this with the NShape library (GPL). **Do not port NShape.** What follows is what the view shows and does, read from Vixen.Application/Setup/SetupPatchingGraphical.cs and Vixen.Application/GraphicalPatching/*. Rebuild it natively, as a single AppKit canvas fed by a layout computed in Rust (see the structure and performance sections).

- [ ] **F111. Layout of the view.** A canvas filling the area, with a button row beneath it (SetupPatchingGraphical.Designer.cs:204-230), left to right: Add Filter (+, tooltip "Add Filter"), Delete Filter ("Delete Filter"), then, after a 10pt gap, Zoom In ("Zoom In"), Zoom Out ("Zoom Out"), Zoom to Fit ("Zoom to Fit") (:143,:155,:174,:186,:198). Canvas background is a vertical gradient from `#FAFAFA` to the theme's text box background colour (SetupPatchingGraphical.cs:84-85, :117; Designer :59-61). NShape's grid is on by default with spacing 19 and colour Gainsboro, snap off (Designer :65-66,:79; Display.cs default `ShowGrid = true`). On the Mac use a `Theme` canvas colour in dark appearance and a subtle grid.

- [ ] **F112. What it displays.** Only what is selected in the two trees, not the whole show (SetupPatchingGraphical.cs:2022-2070):
  - **Elements.** For each selected node, a root shape, except a node all of whose parents (recursively, up to Root) are themselves in the selected set, since it will appear nested inside them (`_allNodeParentsAreInSet`, :268-273). A group shape contains nested shapes for its whole subtree (:296-325).
  - **Filters.** Every output filter reachable through data flow from any leaf of the selected nodes (:333-350). **Upstream bug, not to port:** the leaf list is not de-duplicated (:2029), so a leaf reached twice, which is what a group selected together with one of its own children gives, lists its filters twice, and each gets its own shape with half the height (:369-386, :899-909). The port shows each filter once.
  - **Controllers.** One shape per selected controller, containing one output shape per selected output, sorted by index (:527-590, :641-671).
  - **Lines.** One per real patch between displayed shapes, and dashed stubs where the other end is not displayed (F116).
  Any shape that has left the selection is removed; any that stays keeps its identity and thus its selection and collapse state (:231-263, :369-392, :527-547).

- [ ] **F113. Shape appearance.** Every shape is a rounded box with a vertical two-colour gradient and a centred black title in an 11px system font (FilterSetupShapeBase.cs:92,107). The font shrinks by steps of 0.9 until the title fits 90% of the width or it reaches 6px (:136-143); upstream never grows it back for that shape. A group's title sits centred in its 20pt header (NestingSetupShape.cs:47-56; SetupPatchingGraphical.cs:301). **A title is set once, when the shape is made** (SetupPatchingGraphical.cs:300, :483, :649, :687-690), and a shape that stays in the selection is reused, so upstream shows the old name after any rename of an element, controller or output, until the shape leaves the selection and comes back. The same goes for a reused group's children (:241-256). The port builds the scene from the model each time, so names and children are always current. Colours (top to bottom) (:149-168):

  | Shape | Title | From | To |
  |---|---|---|---|
  | Element group | `node.Name` | 120,160,240 | 90,120,180 |
  | Element leaf | `node.Name` | 200,220,255 | 140,160,200 |
  | Filter | `filter.Name` | 255,220,150 | 230,200,100 |
  | Controller | `controller.Name` | 100,200,100 | 50,200,50 |
  | Output | `output.Name`, or `"{controller} [{i+1}]"` when the name is empty (:687-690) | 180,230,180 | 120,210,120 |

  Stacking order, bottom to top: root element and controller shapes 1, nested shapes 2 and up, filters 10, lines 100 (:303, :313, :489, :656, :693, :1504).

- [ ] **F114. Connection points ("ports").** Drawn permanently on every shape as dots (ConnectionTool.cs:143-157). Inputs on the left edge, outputs on the right edge. Y position for port i of n as a fraction of the shape's height: n = 1 gives 0.5; otherwise `0.1 + 0.8 * i / (n - 1)` (FilterSetupShapeBase.cs:274-297, :304-319). Input count: 1 if the shape has a data flow component, except element shapes which have 0 (:72-75; ElementNodeShape.cs:70-73). Output count: `Outputs.Length`, or 0 when the component or its outputs are null (FilterSetupShapeBase.cs:77-85). Hence: leaf element 0 in, 1 out. Group element and controller shapes 0 in, 0 out (their component is null, ElementNodeShape.cs:60-68, ControllerShape.cs:36-39). Filter 1 in, N out (N changes when its setup changes, for example a Color Breakdown's colour list). Output 1 in, 0 out.

- [ ] **F115. Layout algorithm** (SetupPatchingGraphical.cs:755-991). Runs in view coordinates, with `W` the view's width. Constants: top 10, max shape width 160, min 32, default height 20, min 10, vertical gap 6, child width reduction 10, group header 20, group footer 6 (:733-749). Columns by upstream's comment: elements 5-25%, filters 35-65%, channels 75-95% (:713-719).
  - **Stacks** (elements and controllers). Element width 20% of W, at 5% of W; controller width 20%, at 75%. Roots stack downward from 10 with a gap of 6 (:766-800).
  - **Nesting** (`_ResizeAndPositionNestingShape`, :951-991). Width below 32 is raised to 32. If expanded with children: lay children from `y + 20`, each with `width - 10`, same x, stacked with gap 6. Parent height = `curY - 6 + 6 - y`. If collapsed or childless: height 20 and children are hidden. Note: all children share the parent's `x` as their centre (next point), so they are centred within the parent.
  - **Coordinates quirk.** Shape `X`, `Y` in NShape are the **centre**, and the code assigns the column's left value to `X` (:989-990). So upstream's elements are actually centred at 5% of W (spanning -5% to 15%) and controllers at 75%; `Y` is `y + height/2`. Zoom-to-fit hides this. The port treats the x values as a left edge, which matches the stated columns and avoids negative coordinates. The relative look is the same. Every y in the filter formulas below is still the filter's centre, as `source.Y` is the source's.
  - **Filters** grouped by depth, shallowest first (:802-949). Filter column width `FW` is 30% of W, starting at 35%.
    - Depth 0 (no element source): laid out at the top as a grid. `perRow = max(FW / 38, 1)`, `columns = min(n, perRow)`, column width `FW / columns`, shape width clamped to 32..160 and centred in the column, height 20, `y = 10 + row * 26`.
    - Depth d >= 1: find `source`, the first shape representing the filter's source component, and the siblings sharing that source. Height = `max((sourceHeight - 6 * (siblings - 1)) / siblings, 10)`; y = `source.Y + indexAmongSiblings * (height + 6)` (`source.Y` is the source's centre). If the source shape is not displayed: height 20 and y = `10 + index * 26`. Columns = the deepest filter depth under this filter's ancestral element. Column width `FW / columns`; shape width clamped to 32..160, and if that leaves a margin under 20 while still between the limits, shape width = column width - 20. x = `35% W + (d - 1) / columns * FW + (columnWidth - shapeWidth) / 2`.
    - Depth is found by walking `Source` upward: starting at 1, adding 1 for each filter step, stopping at a displayed element shape; it is 0 if the chain ends with no element (:421-470). **Upstream computes it once, when the filter's shape is made** (:497), so a filter connected later keeps depth 0 and stays in the top grid, and a filter disconnected later keeps its old depth until its shape is recreated. With depth above 0 and no source any more, a window resize throws "null source; this shouldn't ever happen!" (:873-882). The port computes depth at every layout, and a filter with no source is depth 0.
  - **When layout runs.** Element selection or details change: elements and filters, then connections, then zoom to fit. Controller selection or details change: controllers only, then connections, then zoom to fit. Double-clicking a group: that column only. View resize: everything, debounced 250ms (:1872-1897). **So a filter moved by hand keeps its place only until the next element selection change or resize.** Positions are never saved.

- [ ] **F116. Lines.** A straight line from output port to input port with a closed arrowhead at the destination (:1500-1522). Rebuild rules (`UpdateConnections`, :1563-1597; `UpdateShapeSourceConnections`, :1646-1731; `UpdateShapeDestinationConnections`, :1735-1855). For each output and filter shape with a source:
  - Source displayed: a line from **every** shape representing the source (an element shown twice gets two lines to the same input) to this input. Source not displayed: a **dashed stub** with a flat blue cap running 40 units left from the input (:1526-1555, :1718-1728).
  - For each element root and filter, each output with a destination that is displayed gets a real line; if it has destinations but none displayed, a dashed stub 40 units right of the output (:1846-1850).
  - Stale lines are removed.
  **Upstream gap:** only *root* element shapes get the destination pass (:1589-1592), so a leaf nested in a group, patched only to an output that is not displayed, shows no stub. The port runs this for every leaf shape; that only ever adds a stub that conveys true information.

- [ ] **F117. Selecting.** Left click selects the top-most shape or line at the point (ConnectionTool.cs:585-677; `ClicksOnlyAffectTopShape = true`, SetupPatchingGraphical.cs:125). Clicking empty space clears, unless a modifier is held. Ctrl or Shift plus click toggles a shape. A right click with no drag changes nothing (ConnectionTool.cs:450-461). Left drag in empty space draws a selection frame, selecting shapes inside it; a modifier adds to the selection (ConnectionTool.cs:681-700). A left drag that starts on anything that cannot move draws a frame as well (F118). Selection does not change the trees. Selection is preserved across rebuilds for shapes still present (SetupPatchingGraphical.cs:1005-1014). Inserting a shape selects it, replacing the selection (Display.cs:1869-1876), which is why a new filter (F121, F122) or a new line (F119) ends up selected. Stubs (F116) can be selected like lines. Hidden shapes in a collapsed group cannot be hit. Selected shapes show NShape grips; use the system accent outline instead. Upstream's selection colours are DarkGreen normal, Firebrick highlight (SetupPatchingGraphical.Designer.cs:73-76). On the Mac ⌘ or ⇧ toggles; ⌘A selects all (NShape's Ctrl+A, Display.cs:2679-2686).

- [ ] **F118. Moving filters.** Left drag on a filter moves all selected shapes, with a preview during the drag, committing on mouse up (ConnectionTool.cs:863-916). Only filters can move: elements, outputs, controllers and established lines have no layout permission (SetupPatchingGraphical.cs:128-143). If the selection includes anything unmovable the drag becomes a frame select (`IsMoveShapeFeasible`, ConnectionTool.cs:1397-1401, :563). Attached lines follow. No effect on data. **Upstream's Ctrl+Z is not really a move undo.** Ctrl+Z and Ctrl+Shift+Z run NShape's history (Display.cs:2746-2762), and every shape or line the view creates or deletes goes through that same history (DiagramSetController.cs:221-257; Project.cs:428-435), including the ones a selection change rebuilds. So Ctrl+Z undoes the last diagram command, whatever it was: often it removes or restores a shape or line on screen, with the data flow untouched, leaving the picture and the patches in disagreement. Nothing reads this as a feature and nothing else is undone. On the Mac: register only filter moves with the window's `UndoManager`; this is the one undoable action in the window, and undoing it can never change a patch.

- [ ] **F119. Patching by drag, the essential gesture.** Upstream: **right button** drag starting on any shape with outputs (`IsConnectFromShapeFeasible`, ConnectionTool.cs:566-570, :1434-1450; needs connect permission, so leaves, filters only). Start port: the output port under the cursor if there is one, else the first output with no line on it, else output 0 (:717-739). During the drag the free end follows the pointer. On each move (:761-817) find the top-most shape within 5 units, then a port within 10 units; a filter's or output's body also counts, as its reference point allows connecting (FilterShape.cs:69-73; OutputShape.cs:90-94). A hit that is not an input is redirected to input 0, if the shape has an input. Refused while hovering: the source shape itself, and any target for which connecting would form a cycle (`CheckComponentSourceForCircularDependency(target, source)`). On release (ConnectionTool.cs:820-856): with a valid target, delete any other line on that input, `DataFlow.SetComponentSource(target, (source, outputIndex))` which replaces any old source, raise PatchingUpdated, and the line becomes fixed and deletable. It is also left selected (F117), so an immediate Delete removes the patch just made. Otherwise the line is discarded. A connection does not re-lay out the canvas: the shapes stay where they are until the next layout (F115). Undo: Cancel only.
  **On the Mac:** plain left drag starting on an output port does this, since a Mac mouse often has no convenient right button and it is what a node editor does. Keep right-button drag and Control-drag, starting anywhere on the shape, working exactly as upstream. Show a green highlight on the valid target port, and do not highlight refused targets.

- [ ] **F120. Delete.** Delete key with any modifier, or the Delete Filter button (SetupPatchingGraphical.cs:1226-1240, :1974-1980). For each selected item: a line: `ResetComponentSource(destination)`, remove it, raise PatchingUpdated (:1115-1121). **Stubs count as lines (F116), with different results.** Deleting a left stub, on an input whose source is not displayed, really does cut that patch, even though the far end cannot be seen. A right stub, on an output whose destination is not displayed, has no destination recorded, so upstream calls `ResetComponentSource(null)` and throws `ArgumentNullException` (:1537-1545; DataFlowManager.cs:93-98), an unhandled error in the window. The port makes this a no-op with a beep: nothing can say which hidden destination was meant. A filter: disconnect everything on all its ports (each destination's source is reset, its own source reset, raising PatchingUpdated each time), `Filters.RemoveFilter`, remove it (SetupPatchingGraphical.cs:1125-1131, :1394-1455). No confirmation. **Delete Filter's button enable rule:** enabled when any selected shape is a filter, updated only on selection events, so it starts enabled until the first event (:1325-1328). Although the button is named for filters, its handler deletes selected lines too (:1974-1980, `_DeleteShapeAndAssociatedComponents` handles both), which needs a filter in the same selection, since lines alone leave the button disabled. Selecting a filter together with a line attached to it can throw upstream: deleting the filter removes the line as well, and the selection enumerator then indexes past the end (ShapeCollection.cs:1302-1326). Elements, outputs and controllers are ignored. A deleted filter's child filters lose their source but stay on screen until the next element update, then get removed at OK if still unconnected (F6). The canvas does not re-lay out. Undo: Cancel only. Mac: Delete and forward delete, batching the PatchingUpdated event to one per gesture, treating the selection as a set.

- [ ] **F121. Add Filter.** `ListSelectDialog("Add Filter", <every output filter module by TypeName>)`, single select (SetupPatchingGraphical.cs:1940-1972). For the chosen type: create the instance, add to `Filters`, make a shape, width 160, height 20, centred in the current view (:1960-1964). The new filter is **unconnected** and is left selected (F117). Upstream's dialog title reads "Select Item" (F148). Raises FiltersAdded (no consumer). **Unconnected filters are fragile:** the next element selection or details change removes the shape, because display only lists filters reachable from selected leaves (F112), while the filter stays in the system with no source. OK's `RemoveOrphanedFilters` then deletes it (F6). The port keeps those filter shapes on the canvas until the window closes (a `floating_filters` list on the view), since losing a shape you just added is a trap, and OK still removes any left unconnected. Record this in the parity file.

- [ ] **F122. Copy, Cut, Paste filters.** Ctrl+C remembers selected filter shapes in an in-view clipboard, ignoring others, doing nothing if none (:1025-1038). Ctrl+X: copy then delete selected filters and lines (:1256-1271). Ctrl+V: for each clipboard filter, new instance of the same type with a clone of its `ModuleData`, `ModuleDataUpdated` (so ports reflect the data), added to `Filters`, unconnected, placed at the mouse position, spaced downward by height + 6 (:1053-1063, :1134-1186, :1188-1209). So pasted copies have the same fragility as F121, the last one pasted is left selected (F117), and a Cut filter can still be pasted because type and data live on in the old instance. Mac: ⌘C, ⌘X, ⌘V with the canvas focused, via the Edit menu.

- [ ] **F123. Context menu.** Upstream defines "Copy Filter" (enabled when any filter is selected), "Paste Filter" and "Paste Filter (Multiple)" (both enabled when the clipboard is non-empty) (:1303-1323; Designer :111,:118,:125), but **the strip is never attached to the display**, so users cannot reach it (Designer has no `ContextMenuStrip` assignment; `ShowDefaultContextMenu = false` at Designer :77 and SetupPatchingGraphical.cs:124; `ConnectionTool.GetMenuItemDefs` returns null). Paste Filter (Multiple) would prompt `NumberDialog("Number of Copies", "How many copies of the given filter(s)?", 1, 1, 1000)` and paste that many times (:1040-1051). On the Mac add this menu for a right-click without a drag, and Control-click: it costs nothing, and the owner's "keep everything" covers features whose code exists. Record it as a fix in the parity file.

- [ ] **F124. Double-click.** On a selected shape only; upstream's workaround for clicks reaching all shapes under the mouse (:1274-1301). Group element or controller: toggles `Expanded`, hiding or showing children, and re-lays out that column. A collapsed group is one 20pt-high box. Its hidden children are placed at the group's own position, width and height (SetupPatchingGraphical.cs:983-990), and lines are not on any layer, so they stay visible: every line of a hidden child now runs from or to the middle of the group's edge, and the hidden children's port dots are still drawn there (ConnectionTool.cs:146-157). So collapsing never hides a patch, it gathers them. Rebuild it that way: a hidden shape keeps its lines, with their ends at the group edge midpoint. Default expanded (NestingSetupShape.cs:18). Filter: `RunSetup` (`HasSetup` then `Setup()`; on success recalculates ports) (FilterShape.cs:75-84). Filter dialogs are in docs/spec/property-filter-controller-setup.md. Leaf and output: nothing. **Upstream gap:** after a setup that changes the filter's output count, nothing rebuilds lines or raises PatchingUpdated, so lines can dangle until the next selection. The port rebuilds connections and raises PatchingUpdated after a successful setup.

- [ ] **F125. Zoom.** Zoom In: level × 1.1, truncated. Zoom Out: level × 0.9, truncated (SetupPatchingGraphical.cs:1982-1990). Because of the truncation Zoom In cannot leave a level under 10, and Zoom Out from 1 reaches 0; the port keeps the level as a real number, so neither can happen. Zoom to Fit (`ZoomToFitAll`, :1923-1936): `zoom = clamp(min(viewW / contentW, viewH / contentH, 1.0), lower 0.2)`, as a percent, minus 1 when below 100 (a scrollbar workaround the port does not need), then scroll to the top-left. Runs automatically after every element or controller update, so selecting anything re-fits. Ctrl + wheel zooms about the cursor with step sizes by level: below 30 by 1, below 60 by 2, below 100 by 5, below 300 by 10, else 50, within 1..4000 (Display.cs:2387-2411). Plain wheel scrolls; Shift + wheel scrolls sideways (:2413-2417). Arrow keys scroll one small step when no tool consumed them (Display.cs:2627-2647). Mac: two-finger scroll, pinch to zoom about the pointer, ⌘+ and ⌘- when the canvas is focused, keep the auto-fit on selection change. Upstream's other NShape shortcuts (Ctrl+Insert and Shift+Insert for NShape's own shape copy and paste, and Ctrl+Shift+C, Ctrl+Shift+V, Ctrl+Shift+X, which reach NShape because upstream's handler only takes the exact Ctrl chord, SetupPatchingGraphical.cs:1243-1256) would copy shapes with no data behind them; they are accidental and are not reproduced. Shift+Delete does not cut: upstream's handler takes the Delete key with any modifier first (:1226).

- [ ] **F126. Unpatch from the controller tree while this view is showing.** `control_UnpatchControllers` runs Simple Patching's button (F96), then `UpdateConnectionsForControllers` (DisplaySetup.cs:210-215). **Upstream bug:** Simple Patching gets no selection updates while inactive (F3), so the unpatch acts on the controller selection from when Simple was last active, with its stale "enabled" and count. On the Mac, Unpatch always uses the current controller selection through the Rust patching function, and the prompt count is the live one. The tree icons also stay stale after this unpatch, because Simple's PatchingUpdated has no subscriber while it is inactive (DisplaySetup.cs:113-114); the Mac refreshes them. `SetupPatchingGraphical.UnpatchControllers` is a pretend Delete key press (SetupPatchingGraphical.cs:1413-1417) and has no callers in this flow.

- [ ] **F127. Incidental NShape behaviour not reproduced.** Dragging a selected filter's port with the left button resizes the shape through its control point (`MoveHandle`, ConnectionTool.cs:544-549, :923-1027; FilterSetupShapeBase.cs:179-183). Width and height are reset by the next layout and affect nothing else. This gesture is replaced by F119's left-drag from an output port. Input-port left-drag does nothing on the Mac. Recorded here so the parity file marks it deliberately omitted.

- [ ] **F151. Cursor feedback on the canvas** (`DetermineCursor`, ConnectionTool.cs:1265-1315; custom images registered at :1499-1507). Default arrow everywhere, except: over a port of a selected filter when that port's handle can move, a move-point cursor; over a selected movable shape, a move cursor (for the whole drag once moving); while connecting, a hand. **Mac:** `NSCursor.openHand` over a filter and `closedHand` while moving it, `pointingHand` while connecting, and `crosshair` over an output port, since that is where F119's left-drag starts.

- [ ] **F152. Auto-scroll while dragging on the canvas.** Frame select, filter move and connection drags scroll the view when the pointer reaches its edge (`StartToolAction(…, wantAutoScroll: true)`, ConnectionTool.cs:365, :380, :387, :757). No effect on data. **Mac:** call `autoscroll(with:)` on the canvas during those drags.

### I. Export Wire Diagram

Reached from the tree context menu (F52). `ElementModeling.ElementsToSvg(node, flip)` (Vixen.Modules/App/Modeling/ElementModeling.cs:41-73) does nothing, silently, if the node is null, has no leaves, or **any leaf lacks a Location property**.

- [ ] **F128. Export Wire Diagram: options dialog.** `WireDiagramExportOptionsWindow.ShowAsDialog()` (Vixen.Modules/App/Modeling/Views/WireDiagramExportOptionsWindow.xaml, .xaml.cs). Title "Wire Diagram Export Options", 440 x 430, no resize, centred on owner, Esc cancels, Return accepts. Controls top to bottom: checkbox "Show all node numbers" (default on); "Number Label Interval" text box, right aligned, default "50", disabled while the checkbox is on; "Node Marker Size" slider 1 to 30, whole steps, default 6, with the value at the right; "Number Label Size" slider 6 to 24, default 14; "Scaling" slider 100 to 400, default 100, shown as `{n}%`; note text "The diagram is fitted to a letter-sized page. Scaling scales the entire result, including the page, labels, markers, and lines."; OK, Cancel. OK is disabled while invalid (WireDiagramExportOptionsViewModel.cs:154, :206): with the checkbox off the interval must parse as a plain whole number from 1 to 1000, otherwise "Enter a whole-number label interval from 1 through 1000." The accepted options are `LabelInterval = AllNumbers ? 1 : interval`, `LabelFontSize`, `NodeRadius`, `ExportScale = percent / 100` (:226-232). Cancel or closing means no export at all. Values are not remembered between exports (a new view model each time).

- [ ] **F129. Export Wire Diagram: node order and labels** (ElementModeling.cs:41-73, :88-126). Leaves of the one selected group, de-duplicated. If every leaf has an Order property then use `OrderNodes`: if all Order values are distinct, sort by Order; otherwise group by each leaf's first parent inside this group (in order of first appearance, dropping any leaf with no such parent), sorting by Order within each group. Otherwise keep tree order. Each point's label is its Order value if it has one, else its 1-based running position. Position comes from `LocationModule.GetPositionForElement(node)`.

- [ ] **F130. Export Wire Diagram: drawing** (WireDiagramSvgBuilder.cs). An SVG whose width and height are `canvas / 96 * scale` inches and whose viewBox is the canvas (:44-49). Canvas: letter portrait 816 x 1056, or landscape 1056 x 816, whichever gives the larger scale (:313-318), margin 24. Scale and origin per `CalculateLayout` and `GetScale` (:275-331): side guard = `max(1.25 r, r + 2 * maxHalfLabelDiagonal + 4)` on every side, usable size is canvas minus margins minus guards, scale is the smaller of usable/extent per axis, with a zero extent ignored on that axis and 1 when both are zero; centred. Flip (Back Wire Diagram) mirrors x within the fitted extent (:60-63). Elements in draw order: markers in point order, then wires, then labels so nothing covers them (:68-94).

  | Point | Marker |
  |---|---|
  | First | green 5-pointed star: 10 vertices from -90° in 36° steps, alternating radii 1.5r and 0.75r (:378-404) |
  | Last (if not also first) | red square, side 2r (:406-416) |
  | Interval above 1 and label is selected | red diamond, radius r (:418-433) |
  | Other | red circle, radius r (:435-444) |

  Wires: black lines, width 1, consecutive points (:81-84, :343-356). A label is shown for point i when `i == 0 || (i + 1) % interval == 0 || i == count - 1` (:270-273). Labels are Arial, black, at the chosen size, with no rotation. Placement (`PlaceLabel`, :119-160): 16 candidates, the 8 compass directions in the order NE, E, SE, S, SW, W, NW, N, each at two distances `r + projectedHalfSize + 2` and `r + projectedHalfSize + 4`. Keep those inside the page with clearance 2.5. Among those that touch no wire (segment against the label's rectangle inflated by 2.5), pick the least overlap with labels already placed, then nearest start point, then candidate order. If none is clear, the fewest wire crossings first, then the same tie-breaks. If none is in bounds, nearest then candidate order. Overlap uses a rectangle of `0.75 × label size` (:187-197). Text width was measured by GDI+ `MeasureString` + 2, and ascent and descent from the font's cell metrics + 2 each (:99-117); the Rust builder uses Arial's metrics (digit advance 0.556 em, ascent 0.905 em, descent 0.212 em), so exact coordinates may differ slightly from Windows while placement rules stay the same.

- [ ] **F131. Export Wire Diagram: result.** Writes to a temp file with an `.svg` extension and opens it with the default application (ElementModeling.cs:75-86). Mac: write to the app's temp directory and `NSWorkspace.shared.open`, which opens the default SVG viewer, normally the browser. Nothing else happens, no data changes, no undo. Note a silent no-op when any leaf has no Location property (F52); the Mac version shows an informational alert, "Every element in the group needs a Location property to export a wire diagram.", since silence looks like a broken menu item. Record that in the parity file as an addition.

### J. Manage Tag Colors window

- [ ] **F132. Manage Tag Colors** (Vixen.Common/ElementTagManager). Opened from Tags > "Manage Tags..." (F56). Title "Manage Tag Colors", 360 x 260, not resizable, centred on screen, Esc cancels, Return saves (ElementTagManagerWindow.xaml:11-13, :26-27). One row per **built-in** tag in `SortOrder` (ElementTagManagerWindowViewModel.cs:25-29), so Deprecated, Hidden, Prop: the name at left, a 32 x 24 colour swatch with a border at right (.xaml:38-46). User tags are not listed yet, although the service supports them. **Double-click** a swatch to open the colour picker, starting from the staged colour (.xaml:43-45; TagColorItem.cs:63-72). Edits are staged. Buttons: "Save", "Cancel". Save writes each staged colour to its definition as `#RRGGBB` upper-case hex (TagColorItem.cs:78-81). Save from Display Setup passes `saveOnClose: false`, so nothing is written to disk until Display Setup's OK, and Display Setup's Cancel reverts it (SetupElementsTree.cs:367-376). Other callers (the Sequencer) pass true, which also saves the system config (ElementTagManagerWindowViewModel.cs:85-88). A tag's bad or empty colour shows as white (TagColorItem.cs:83-96). After Save, the tree repaints its dots. Mac: a sheet, with SwiftUI `ColorPicker` swatches (single click opens the system colour panel), and `.help` is not needed since upstream has no tooltips here.

### K. Naming dialogs (used by Add Multiple, Rename and Find/Replace)

These two dialogs are generic controls in Vixen.Common/Controls/NameGeneration. They are specified here because the element tree is their main client. The rules live in Vixen.Core/Rule/Name.

- [ ] **F133. Create/Modify Multiple Items (`NameGenerator`).** Title "Create/Modify Multiple Items", 699 x 646, minimum 715 x 684 (NameGenerator.Designer.cs:560-566). Three modes (NameGenerator.cs:38-83): new names, seeded with a text (Add Multiple, F41), one "Name" column; rename, given old names, two columns "Old Name" and "New Name", seed "NewName", count fixed to the number of old names and disabled (:60-71, :19-34); fixed count, unused here. Controls: a "Use template:" label and combo that are **hidden** (`Visible = false` in the Designer, and nothing shows them), so the only template is loaded silently on open (F134) and the Mac shows neither; "Total Number of Items:" number 1 to 1,000,000, default 5 (Designer :179-183); "Name format:" text with the hint `Example: "Tree - <1> - <2> - <3>"`; group "Naming Rules" with a rule type combo and + button, a rule list with up (U), down (D), delete (-) buttons, and a panel showing the selected rule's editor; the preview list on the right; OK, Cancel. Every change regenerates the preview live (:186-189, :417-431). Upstream has no tooltips here.

- [ ] **F134. Template and rule list behaviour.** The only template is "Numbered Items": format `-<1>`, one Sequential Numbers rule starting at 1, step 1, endless (NumberedNamingTemplate.cs). On load it is selected, so the format box becomes `{seed}-<1>` and the first rule is selected (NameGenerator.cs:85-100, :173-184). Rule rows read `<{n}> {rule name}`. Rule types: Sequential Numbers, Sequential Letters, List of Letters, List of Words, listed in reflection order with the first selected (NameGenerator.cs:87-90). By the reasoning in F99 that order is likely Sequential Letters, List of Letters, Sequential Numbers, List of Words, an inference; the Mac uses that fixed order. Add: appends a fresh rule, selects it, and if the format has no `<n>` yet, appends ` <n>` to it (:287-313). Delete: removes it, selects the previous row (or the row now at index 0), and always strips the placeholder of the old **highest** rule number, with or without a leading space (:315-348). The other placeholders are not renumbered, so deleting a rule from the middle shifts what they mean. Up and Down swap with the neighbour; the format text does not change, so placeholders then refer to the new order (:350-381). Move and Delete are enabled when a rule is selected (:168-170). Add is enabled when a type is selected (:282-285), which makes its "Select a rule type first." warning (title "Warning", :289-296) unreachable.

- [ ] **F135. Name generation algorithm** (`GenerateNames`, NameGenerator.cs:231-279). Inputs: format text, ordered rules, wanted count N (the old names' count, or fixed count, or the number box, :191-202). Recursive at depth d from 1: no rule at d, or a count so far already above N, gives nothing. For each value in rule d's cycle (endless rules run on until stopped): replace every `<d>` in the format with the value; at the last rule add the string; otherwise recurse with depth + 1 and the count so far, and if the recursion gives nothing add the string as it stands. Stop as soon as the count so far reaches N. Thus multiple rules behave like an odometer with the **last rule fastest**, and finite rules can yield fewer than N. With no rules at all, the result is empty. A format with no placeholder yields repeats, which `Uniquify` then differentiates. In rename mode rows beyond the list show `-` in New Name and keep their old names (:215; ElementTree.cs:905-911).

- [ ] **F136. Rule types.**
  - **Sequential Numbers** (`NumericCounter`). Fields: "Start Number:", "End Number:" (disabled when Endless), "Endless" checkbox, "Step:"; each -1,000,000 to 1,000,000. Defaults 1, 5, endless on, 1. Value at i is `start + step * i`. Cycle length `floor((end - start) / step) + 1`. Treated as endless when Endless is on, or when step and range disagree in sign (NumericCounter.cs:29-32). Step never becomes 0: typing 0 jumps to -1, or to 1 if the previous step was negative (NumericCounterEditor.cs:42-53).
  - **Sequential Letters** (`LetterCounter`). "Start Letter:" text accepting only letters, first character used, case sets output case, default A. "Steps:" 1 to 1,000,000, default 5. Generation is base 27 over the alphabet string `aabcdefghijklmnopqrstuvwxyz`, starting from `start index + 1 + i` (LetterCounter.cs:42-57). **Quirk to keep:** after Z come AA, AA, AB, ... with AA repeated, since index 0 and index 1 are both "a". Expected from A: A, B, ..., Z, AA, AA, AB. `Uniquify` differentiates the duplicate.
  - **List of Letters** (`LetterIterator`). Label "Enter a list of Letters to be use", hint "Example: LCR LR NS EW UL (Only one)". Non-alphanumerics are dropped (LetterIterator.cs:51-55). Default "ABCDE". One name per character, wrapping by modulus. Clearing the box keeps the previous letters (LetterIteratorEditor.cs:22-28).
  - **List of Words** (`WordIterator`). Label "Enter a list of Words seperated by commas" (sic), hint "Examples: Left, Right, Upper, Lower". Split on commas, each trimmed, with empty entries kept, so "a,,b" is three words (WordIteratorEditor.cs:22-28). The model's default list is empty (WordIterator.cs:7), a zero-length cycle. But the Designer's text box holds one space, so as soon as the rule's editor shows (and adding a rule shows it) `Load` sets the text to empty, `TextChanged` fires, and the list becomes one empty word (WordIteratorEditor.cs:17-28). In practice an untouched List of Words rule therefore has one iteration that substitutes nothing. Keep this result.

- [ ] **F137. Find / Replace Name (`SubstitutionRenamer`).** Title "Find / Replace Name", 708 x 508, centred on parent (SubstitutionRenamer.Designer.cs:315-319). "Patterns" label, list with U, D, +, - buttons; "Find What:" and "Replace With:" fields editing the selected pattern; preview list "Old Name", "New Name"; OK, Cancel. Starts with one empty pattern, selected (SubstitutionRenamer.cs:40-47). Row text `#{n} Replace '{find}' with '{replace}'`, or `#{n} Nothing to find.` when Find is empty (:105-108, :288-297). Replace is ordinal and case-sensitive, all occurrences, patterns applied in order, each on the previous result, empty Finds skipped (:110-135). OK is disabled until any pattern has a non-empty Find, and this is re-evaluated only when Find is edited (:28, :177-183). Delete does nothing when only one pattern remains (:202). Deleting a pattern selects the previous row; deleting the first row leaves nothing selected (:208-212). With no selected pattern the two fields are disabled (:217-227), and U and D upstream throw, because they read the selected index before checking it (:250, :266); the Mac disables U and D when nothing is selected.

- [ ] **F138. Naming rule summary, for reference.**
  - Node names are unique across the whole system, case-sensitively, by `{name} - {n}` from 2 (NodeManager.cs:123-135). The invisible Root counts, so a new "Root" becomes "Root - 2". Nothing trims a name: a name of spaces is accepted, and only an exactly empty response takes the defaults below.
  - The element's name always equals its node's.
  - **Upstream bug, fixed in the port:** `RenameNode` uniquifies against all nodes including the node itself, so renaming to the same name yields "X - 2". The single-rename path guards against this (ElementTree.cs:934) but Find/Replace and bulk rename do not (:912, :957), so **a Find/Replace that does not match one of the selected rows renames that row to "X - 2"**. Rust's rename treats a name equal to the node's current name as a no-op, and excludes the node itself from the uniqueness check.
  - Controller names have no uniqueness rule, and may duplicate.
  - Empty responses: Add gives "New Element", Single Item gives "New Item", Rename ignores it, controller name falls back to the type name, controller rename ignores it.

### L. Intelligent Fixture Wizard (the Intelligent Fixture template's dialog)

Vixen.Modules/Editor/FixtureWizard/Wizard. Upstream uses Catel's Orc wizard: a left "bubble" navigation listing the pages, with Back, Next, Finish, Cancel and Help. The fixture-profile editing pages embed the fixture property editor (Editor/FixturePropertyEditor), which is outside this spec: **Edit Functions** hosts `FunctionTypeView`, and **Edit Channels** hosts `FixturePropertyEditorView` with its Load button hidden.

- [ ] **F139. Wizard window.** Title "Intelligent Fixture Wizard". Minimum 1100 x 605, maximum 1400 x 605, resizable, shown in the task bar, help button visible, quick navigation between already visited pages allowed, navigation states handled, views not cached (IntelligentFixtureWizard.cs:37-58; IntelligentFixtureTemplate.cs:918-930). Pages in order: Select Profile, Edit Functions, Edit Channels, Color Support, Automation, Dimming Curves, Grouping, Summary (IntelligentFixtureWizard.cs:40-47). Each page has a title and a description line. Dark colour scheme with DodgerBlue accent (IntelligentFixtureTemplate.cs:1231-1239). Help opens a per-page web page, with the base path `http://www.vixenlights.com/vixen-3-documentation/setup-configuration/setup-display-elements/intelligent-fixture-wizard/` and leaves `select-profile/`, `edit-functions/`, `edit-channels/`, `color-support/`, `automation/`, `dimming-curves/`, `grouping/`, `summary/` (each page model's constructor; IntelligentFixtureWizard.cs:136-152). Finish returns true; Cancel returns false, which sets `Cancelled` so nothing is created. The navigation controller derives from Orc's `FastForwardNavigationController` and only restyles the four buttons (FixtureWizardNavigationController.cs:10, :55-106); whether Orc enables Finish before the last page could not be read (its source is not in the repo), so check the real window on Windows or ask the owner. Until then the Mac enables Finish on Summary only. **Pages are rebuilt on every visit** because views are not cached: each page's view model constructor runs again each time the page opens, so the defaults in F143, F144 and F146 are re-applied when the user comes back to a page. Mac: a separate window (a long task you work in), with a sidebar of steps at left, and Back, Next or Finish, Cancel and Help buttons at the bottom. Keep the same step order.

- [ ] **F140. Decorative moving heads.** Most pages draw a static 3D moving head at each side of the content, "strictly to add visual interest" (WizardUserControlBase.cs:8, :35-66): Select Profile DodgerBlue 300, Color Support Blue 400, Automation Purple 400, Dimming Curves Pink 400, Grouping Green 300. Not functional. The port omits them for now. Recorded under omissions.

- [ ] **F141. Page: Select Profile** (SelectProfileWizardPageView.xaml; SelectProfileWizardPageViewModel.cs). Description "Select or create the profile for the fixture(s)." Group "Select Profile": radio "Select Existing Profile" with a combo of all saved profile names at its right (combo enabled only when this radio is on; radio itself enabled only if at least one profile exists, :44-51), radio "Create New Profile". With profiles available, the existing radio is default and the first profile is selected (:161-165); with none, Create New is default. Picking a name clones that profile (`CreateInstanceForClone`, :95); choosing Create New makes a blank profile with built-in functions (`InitializeBuiltInFunctions`, :101-104). Group "Profile Properties": "Profile Name:" (tooltip "Enter a unique name for the fixture profile."), "Manufacturer:" ("Enter the manufacturer of the fixture."), "Created By:" ("User that created the fixture profile."), "Revision:" ("Revision of the fixture profile."). Validation: an empty Profile Name gives "Profile Name is empty.  Profile Name is a required field." shown in a validation bar, and blocks moving on (:334-344). The name is not checked for uniqueness despite the tooltip: on finish, saving under an existing name overwrites that profile (FixtureSpecificationManager.cs:213-228), and profiles stay sorted by name. Summary line: the selected fixture name.

- [ ] **F142. Pages: Edit Functions, Edit Channels.** Descriptions "Define the fixture functions." and "Define the fixture's channels." Edit Functions shows the function editor with the profile's functions, initially selecting "Pan" (EditProfileFunctionsWizardPageViewModel.cs:31). On leaving, it writes the functions back (:70-86). Edit Channels shows the channel editor on the profile; on leaving it writes the profile back and sets `SelectedFixture` to the profile's name (EditProfileWizardPageViewModel.cs:70-92). Back and Next are blocked while the editor reports validation errors (`CanMoveBack`, `CanMoveNext`, :137-148; IntelligentFixtureWizard.cs:180-221). Editor details belong to the fixture editor's spec. Neither page has a summary line.

- [ ] **F143. Page: Color Support** (ColorSupportWizardPage.cs; ColorSupportWizardPageView.xaml). Description: "Select the color support applicable to the fixture.  Color Mixing should be selected when the fixture uses a multiple color light source or multiple light sources of different colors.  Color wheel should be selected when the fixture has a single white light source and is equipped with a color wheel to change colors.  " Group "Color Support" radios and tooltips:
  - "No Color Support": "Select if you want to manually control color through separate display elements.  This option is not recommended."
  - "Color Mixing": "Select when the fixture uses multiple color source or multiple light sources of different colors.  Examples are RGB and RGBW"
  - "Color Wheel": "Select when the fixture has a single white light source and is equipped with a color wheel to change colors."
  Default from the profile: any channel on an RGB or RGBW function gives Color Mixing; else any on a Color Wheel function gives Color Wheel; else No Color Support (ColorSupportWizardPageViewModel.cs:26-40; FixtureSpecification.cs:362-404). The default is re-applied on every visit (F139). Summary: "No Color Support", "Color Mixing" or "Color Wheel".

- [ ] **F144. Page: Automation** (AutomationWizardPage.cs; AutomationWizardPageView.xaml; AutomationWizardPageViewModel.cs). Description "These options configure special filters that make it easier to control the fixtures when sequencing." Group "Automation Options" with four checkboxes, whose enabling and defaults are worked out when the page opens:

  | Checkbox | Tooltip | Enabled and default on when |
  |---|---|---|
  | Automatically Open and Close Shutter | "This option will automatically control the shutter channel based on the color channels.  Whenever an effect is generating color, the shutter will open.  Whenever there is no effect, or the effect is dark, the shutter will be closed." | colour support is not None, and the profile uses a Shutter function (`SupportsFunction(FunctionIdentity.Shutter)`, :31-42) |
  | Automatically Control Color Wheel | "This option will automatically control the color wheel by matching colors generated in the effects to colors supported on the color wheel." | Color Wheel chosen on the previous page (:45-58) |
  | Automatically Control Dimmer | "This option is usually selected for fixtures with a color wheel and fixed white light source.  It will automatically control the fixture's dimmer channel based on the intensity of the colors in the effects." | the profile uses a Dim function (:71-79) |
  | Automatically Open and Close Prism | "This option is usually selected for fixtures that both a channel for enabling the prism and another channel for selecting the position or movement of the prism." | the profile uses an OpenClosePrism function (:61-68) |

  Summary: the enabled items joined, for example "Shutter, Color Wheel, Dimmer,Prism" (sic: the Dimmer item has no space after its comma; trailing separators trimmed, AutomationWizardPage.cs:54-85). In the template, Automatically Open and Close Prism is read but ignored (F110). Because the constructor only ever sets these true (AutomationWizardPageViewModel.cs:21-80), a box unticked earlier comes back ticked on return to the page, and one that became disabled keeps its old value: choosing Color Wheel, moving on, then going back to Color Mixing leaves Automatically Control Color Wheel true and disabled, and it still reaches generation. The Mac treats a disabled box as false.

- [ ] **F145. Page: Dimming Curves** (DimmingCurveWizardPage.cs; ViewModel; View). Description "These options configure if any dimming curves are applied to the fixture outputs." Group "Dimming Options" radios and tooltips:
  - "Do Not Include a Dimming Curve" (default): "Select when dimming curves are not desired."
  - "Add Dimming Curve For Fixture": "This option adds a single dimming curve for all color channels."
  - "Add Dimming Curve For Each Color Channel": "This option configures a separate dimming curve for each color channel.  With this option all the dimming curves are initially identical.  The curves can be further adjusted via the Patching Graphical View." Enabled only when Color Mixing was chosen (DimmingCurveWizardPageViewModel.cs:29).
  Button "Setup Dimming Curve" opens the curve editor on the page's curve, replacing it on OK (DimmingCurveWizardPageViewModel.cs:42-55); the curve editor is the shared one specified in docs/spec/property-filter-controller-setup.md. Initial curve is `new Curve()`. Summary: "No Dimming Selected", "Dimming Curve for Fixture" or "Dimming Curve for Each Color Channel".

- [ ] **F146. Page: Grouping** (GroupingWizardPage.cs; GroupingWizardPageView.xaml; GroupingWizardPageViewModel.cs). Description "Configure how many fixtures to create and if they should be grouped."
  - "Number of Fixtures to Create:" (tooltip "Number of fixture display elements to create.") a read-only number, default 1, with + button ("Select to increment the number of fixtures to create.") and - button ("Select to decrement the number of fixtures to create."), the latter disabled at 1 (:98-102).
  - "Element Prefix:" ("Name prefix used in the display element tree for the fixtures."), default the profile name.
  - Checkbox "Create Group" ("Select to nest the fixtures under a group in the display element tree."), default on.
  - "Group Name:" ("Name of the fixture group in the display element tree."), default "Fixtures", enabled only with Create Group.
  - Group "Display Elements Preview" at right: a tree, always expanded, each row the name, with a green bullet on leaves only (FixtureWizardTreeItem.cs:30-37). With a group: root = group name, children = `{prefix}{n}`. Without: only the fixtures (GroupingWizardPageViewModel.cs:229-252). Names update live as the fields change (:201-218, :262-273). All of these defaults are set in the constructor (:32-72), so coming back to this page puts the count to 1, Create Group on and Group Name "Fixtures" again, and the prefix back to the profile's name (F139).
  The Mac's preview is a small SwiftUI `List`/`OutlineGroup`; count is small enough. No upper bound on the count upstream; keep it.
  Summary lines: `# Fixtures: {n}`, `Group Name: {name}` (with a group), `Element Prefix: {prefix}` (GroupingWizardPage.cs:30-46).
  **Note:** the first preview child is hard-coded "Fixture_1" until the prefix setter runs, which happens in the same constructor (GroupingWizardPageViewModel.cs:50, :66), so it is never seen.

- [ ] **F147. Page: Summary.** Description "Below is a summary of the fixture selections." Shows each page's title with its summary text (`SummaryWizardPageBase.GetSummary`). Finish is available here; quick navigation can return to any visited page. The Finish button starts generation (F110).

### M. Shared prompts

- [ ] **F148. `TextDialog`, `NumberDialog`, `ListSelectDialog`.** Used throughout this window.
  - `TextDialog(prompt[, title[, initial, selectAll]])`: one text field, focused, initial text selected when asked; Return accepts, Esc cancels (Vixen.Common/Controls/TextDialog.cs:20-44). The one-argument form used by F40 and F45 has a blank title bar, as the Designer sets none.
  - `NumberDialog(title, prompt, value, min = 0, max = int.MaxValue)`: a number field; Return accepts, Esc cancels (NumberDialog.cs:7-34). The default bounds matter: Channel Count (F88) and Insert Channels (F94) accept 0 and ignore the module's output limit, which only the Add flow enforces (F77). Inserting 0 is a no-op.
  - `ListSelectDialog(title, items)`: a list with no initial selection; OK is disabled until a row is selected; double-click or Return accepts; Esc cancels (ListSelectDialog.cs:56-92). **The title argument is stored and never used** (:8, :13), so upstream's title bar always reads the Designer's "Select Item" (ListSelectDialog.Designer.cs:114). The Mac uses the title the caller meant: "Add Property", "Add Filter". Used in single-select mode by Add Property (F17) and Add Filter (F121), listing modules in descriptor order. On the Mac, list them sorted by name.

### N. Register of deliberate changes from upstream

Everything else in this spec follows upstream. The owner can veto any of these. Each needs a line in `docs/parity/display-setup.md`.

| Entry | Upstream | Mac |
|---|---|---|
| F7 | ⌘W equivalent, Alt+F4, loses all edits silently | Cancel button and Esc stay silent; the close box and ⌘W ask "Discard changes to the display?" only when the session is dirty |
| F150 | Esc during a canvas drag closes the window and loses all edits | Esc cancels only the drag |
| F17, F121, F148 | List dialog's title bar always reads "Select Item" | Sheet titled "Add Property" or "Add Filter" |
| F44 | Deleting a group row with rows under it leaves a ghost element in the system; one event per deleted row | Overlapping rows are skipped, nothing is created; one event per gesture |
| F77 | New controller keeps old output rows in the logical selection | Only the new controller is selected |
| F90 | Panels keep showing a deleted controller until the next selection | Selection cleared, panels refreshed |
| F112 | Filter drawn twice when its leaf is reached twice | Each filter once |
| F113, F115 | Shape titles, children and filter depth go stale while a shape stays selected | Scene built from the model each time |
| F120 | Deleting a right-hand stub throws; deleting a filter with its own line selected can throw | Right stub ignored with a beep; selection handled as a set |
| F125 | Zoom buttons truncate and can get stuck under 10% | Zoom kept as a real number |
| F137 | U and D throw with no pattern selected | Disabled |
| F144 | A disabled automation box can keep a stale true | Disabled means false |
| F33 | Ctrl+X can delete rows until the menu is first opened | Cut stays disabled everywhere |
| F35, F37 | A copied node deleted before pasting comes back as a dead leaf | Deleted nodes are skipped; beep if none left |
| F40 | Declining the convert prompt crashes | Treated as cancel |
| F41 | Add Multiple converts a patched leaf with no warning | The same F43 prompt, once, before creating |
| F47, F51 | Find/Replace and Sort don't notify others | The patching view and tree refresh |
| F57 | Drops don't refresh patch status | Status and patching view refresh after each drop |
| F71 | Plain click on a row in a multi-selection keeps the selection; click left of the text, including the expand glyph, clears | Standard `NSOutlineView`: click reduces to that row on mouse up; a click anywhere on a row selects it; the disclosure triangle leaves the selection alone |
| F73 | Type-select misses lower-case names | System type-select |
| F75 | Drop silently refused if target's text path starts with the dragged row's | True ancestor test |
| F153 | Rows can be dragged between the trees: an element row lands among the controllers, a controller row crashes | Each tree accepts only its own rows |
| F78, F79 | Controller order changes reach the model at OK and repopulates lose them; any row draggable | Applied to the working copy at once; only root rows drag; no device restarts at save |
| F105 | Pasted bad pattern throws | Same warning alert |
| F110 | Fixture delete-list persists between runs and can crash | Per-run result |
| F116 | Nested leaves get no stub for an undisplayed destination | All leaves get stubs |
| F118 | Ctrl+Z undoes the last NShape diagram command of any kind and can leave the picture disagreeing with the patches | `UndoManager` for filter moves only |
| F119 | Connect only by right-drag | Left-drag from an output port also connects; right-drag and Control-drag stay |
| F121, F122 | Unconnected filters vanish from the diagram at the next selection change, then get deleted at OK | They stay visible until the window closes; still deleted at OK if unconnected |
| F123 | Filter context menu defined but unreachable | Attached |
| F124 | Filter setup leaves stale lines | Connections rebuilt, patching updated |
| F126 | Tree's Unpatch uses a stale selection while Graphical is showing, and tree icons do not refresh | Uses the live selection, and icons refresh |
| F127 | Port drag resizes filter | Omitted |
| F131 | Export does nothing if a leaf has no Location | Informational alert |
| F138 | Bulk rename or Find/Replace of an unchanged name appends " - 2" | Unchanged names stay |
| F140 | Decorative 3D heads | Omitted |

## Data and persistence

### The live model and when it reaches disk

Everything in this window edits the in-memory system (nodes, elements, properties, data flow, filters, controllers) directly. Nothing is transactional. **OK** runs `ApplyConfirmedChanges` then `SaveSystemConfigAsync` then `SaveModuleConfigAsync`; **Cancel** runs `ReloadSystemConfig` (VixenApplication.cs:991-1026). So the Rust side needs: load the system model from `SystemData`, mutate it freely, then either write it all or throw it away and reload. The exact file names and XML shapes of the system and module configuration are in crates/vixen-io and crates/vixen-xml and are not restated here. The items this window writes are:

| What | Held on | Written at |
|---|---|---|
| Element tree: names, parent and child links, order, node ids, element ids, tags. Elements are saved as their own list, so an element with no node (F44) is saved too | `ElementNode`, `VixenSystem.Nodes`, `VixenSystem.Elements` | system config on OK |
| Properties on nodes, and their data | `ElementNode.Properties`, `VixenSystem.ModuleStore.InstanceData` | module config on OK |
| Data flow patches: for each component, source component id and source output index | `DataFlowManager` | system config on OK |
| Output filter instances | `FilterManager` plus their module data | system config and module config on OK |
| Controllers: id, name, module, output list with output ids and names, order | `OutputControllers` | system config on OK |
| Controller module settings | module data | module config on OK |
| Tag definitions, including `DisplayColor` | `VixenSystem.TagDefinitions` | system config on OK (F132) |

Not persisted: active patching view, radio and checkbox values, expansion, selection, scroll positions, combo choices. Those last for the life of the window, or less.

### Element nodes (Vixen.Core/Sys/ElementNode.cs, GroupNode.cs)

- A node has a GUID `Id`, a `Name`, ordered `Children`, a `Parents` list, optional `Element`, `Properties`, `Tags` (ElementNode.cs:20-37; GroupNode.cs:7-18). Multi-parent is normal, and the tree is really a DAG under an invisible `RootNode` named "Root" (NodeManager.cs:35).
- `IsLeaf` means no children (ElementNode.cs:135-138). A leaf normally has an `Element`, a group normally does not. The element's name is kept equal to the node's name (ElementNode.cs:29-33, :94-104).
- Each `Element` has its own GUID. Its data flow component id is the element id (ElementDataFlowAdapter.cs:63-66), and it has exactly **one output** (:18-27).
- `AddChild` and `InsertChild` do nothing when the child is already present (GroupNode.cs:27-41).
- `InvalidChildren()` of a node is: itself, its direct children, and all its ancestors via every parent (ElementNode.cs:145-160). `InvalidRootNodes` is that for `RootNode`, which amounts to the current roots plus Root itself (NodeManager.cs:137-140).
- `GetLeafEnumerator` is depth-first in child order, a leaf gives itself; a node under several parents appears once per path, callers `Distinct()` it (ElementNode.cs:258-266; NodeManager.cs:198-203).
- `GetAllParentNodes` recurses through all parents without a visited set (ElementNode.cs:288-291), so it can revisit shared ancestors.

### NodeManager rules

- **Naming.** `_Uniquify(name)` (NodeManager.cs:123-135): if any node already has exactly that name (case-sensitive, across every node in the system, including the node being renamed), try `"{name} - 2"`, `"{name} - 3"`, ... until unused. `CreateSingle` defaults a null name to "Unnamed" (ElementNodeService.cs:23). `AddNode(name, parent, uniquifyName)` uniquifies by default. `RenameNode` always uniquifies and copies the result to the element (NodeManager.cs:96-101). So there are no duplicate node names unless a caller passes `uniquifyName: false` or files already contain them, and templates that ask for "X-1" under a second copy of the template get "X-1 - 2". The invisible Root node is an ordinary entry in that list (ElementNode.cs:23-27; NodeManager.cs:35), so a new element typed as "Root" becomes "Root - 2".
- **`AddChildToParent(child, parent, index = -1)`** (:103-121): null parent means root. If the parent has an element, that element is removed from the system **and with it all its patches** and the parent becomes a group. Then append, or insert at index.
- **`RemoveNode(node, parent, cleanup)`** (:77-94): null parent means root. Removes the link. If the parent (not root) now has no children and no element, it gets a brand-new `Element` with its own name and fresh id, so the emptied group is a white-ball leaf again. With `cleanup` true and no parents left on the node (GroupNode.cs:57-64, ElementNode.cs:190-200): recursively unlink its children (which survive if they have other parents), remove its element, remove all its properties, and forget its id. `cleanup` false leaves children attached (for moves).
- **`MoveNode(node, newParent, oldParent, index = -1)`** (NodeManager.cs:37-53): null means root on either side. If moving within a parent and `index > current index`, first decrement the index, since the removal shifts things. Then `RemoveNode(node, oldParent, false)` and `AddChildToParent(node, newParent, index)`. Moving the only child within its parent makes a throwaway `Element` for a moment; the port's Rust move does not need that.
- **`ExportElementNodeProxy(path)`** (NodeManager.cs:168-177) writes `new ElementNodeProxy(RootNode)` through `ElementTreeWriter` (JSON, per the `Vixen.IO.JSON` namespace). Its counterpart `ImportElementNodeProxy` is used by the sequence mapper, not here.
- **`ElementManager`:** `AddElement` registers the element and its data flow component; `RemoveElement` removes both, which cuts every patch from that element (ElementManager.cs:47-75, :129-137).

### Data flow rules (DataFlowManager.cs)

- Each component has at most one `Source` (a component and an output index); an output may feed any number of destinations; `_componentDestinations` is the reverse map.
- `SetComponentSource(component, source)` refuses (returns false) when following `source`'s chain upward reaches `component`, so cycles cannot be made (:71-81, :170-188). Otherwise it detaches the old source and attaches the new (:190-205). Simple Patching ignores the return value.
- `ResetComponentSource` detaches only (:93-98).
- `RemoveComponent` detaches all its destinations, then its own source, then forgets it (:130-138).
- `Initialize` restores patches at load, ignoring those whose child is missing or whose output index is out of range (:24-34). It would dereference a missing source component; the Rust loader skips such a patch and logs it.
- Controller outputs have `Outputs == null` (CommandOutputDataFlowAdapter.cs:18-21), which is what ends patch-point recursion (SetupPatchingSimple.cs:320).
- A component's destinations are kept in connection order, and saving writes patches by walking components and their destinations (DataFlowManager.cs:228-231). So patch order in the file decides the order of an element's filters after a reload, and with it the order of its patch points (F61, F69). `to_patches` must preserve this.
- Controller output component id is `CommandOutput.Id`, an output-specific GUID (CommandOutputDataFlowAdapter.cs:33-36).

### FilterManager rules

- Filters are data flow components. Adding or removing a filter is `DataFlow.AddComponent` / `RemoveComponent`, with the module data assigned or removed in reaction (FilterManager.cs:33-41, :140-143, :162-165).
- `GetOrphanedFilters`: filters that are not roots and have no source (FilterManager.cs:184-190). `RemoveOrphanedFilters` removes each with its filter-children chain, depth first (:192-207). This is run at OK (F6).
- `_rootFilters` and `_filterChildren` exist for the frame engine; those are crates/vixen-output's business.

### Output controllers

- `OutputController.OutputCount` setter (Vixen.Core/Sys/Output/OutputController.cs:233-249): growing adds outputs named `"Output {n}"` (1-based) with indexes in order; shrinking removes the last outputs one at a time, each removal doing `DataFlow.RemoveComponent` (so its patch is lost) and dropping the lookup entry (:329-336). Names of remaining outputs do not change.
- `InsertOutputsAt(index, count)` (:268-298): new outputs land at `index..index+count`; existing outputs from `index` on are re-appended with new indexes, keeping identity and patches. Then `UpdateOutputNames` (:300-313): module-provided names when `SupportsNamedOutputs`, else all outputs renamed `"Output {i+1}"`. Raises OutputCountChanged.
- `RemoveOutputs(list)` (:338-351): removes each, re-indexes the rest without touching their identity or patches, renames as above, raises OutputCountChanged.
- `OutputControllerManager.Remove(controller)`: `Stop`, `OutputCount = 0`, then remove from the collection (OutputControllerManager.cs:182-187).
- `Reorder(sortOrder)` (:219-244): sorts by index in the list (ids missing from the list get -1 and go first), removing and re-adding each controller, then restoring stopped or paused state when the system is started, because adding auto-starts.
- `getOutputDetailsForDataFlowComponent` is a dictionary lookup from the output's data flow component to (controller, index) (:80-97), which is `UpdateControllerOutputIndex`'s reason for existing. `GetDataFlowComponentForOutput(controller, index)` on the manager has an off-by-one (`outputIndex > 0`, :41) that makes it return null for output 0; the panels use the controller's own method, not this. Do not port the bug.

### Tags

- `ElementTagDefinition`: `Id` (stable GUID), `Key` (semantic key for built-ins, empty for user tags), `Name`, `IsBuiltIn`, `Description?`, `DisplayColor?` (hex string), `SortOrder` (ElementTagDefinition.cs).
- Built-ins, fixed ids, in this order (BuiltInElementTags.cs:50-76): Deprecated `753115da-e27a-4261-a136-8222ccc3f22e`, key "deprecated", `#FF0000`, sort 0. Hidden `2f21bfcb-07a3-4cac-bdc5-4b6e37773c04`, key "hidden", `#000000`, sort 1. Prop `6db1852d-71d8-449d-afa5-193a6def26a0`, key "prop", `#0000FF`, sort 2.
- `ElementTagService` (Vixen.Core/Services/ElementTagService.cs): `GetAll` orders by SortOrder then name ordinal-ignore-case. `CreateUserTag(name)`, `RenameUserTag(id, name)`: names are trimmed, must not be blank ("Tag name cannot be blank.") and must be unique case-insensitively (`A tag named "{name}" already exists.`). Rename and delete of a built-in throw. `DeleteUserTag` removes the tag from the catalog and from every node's `Tags`.

### Properties on nodes (Vixen.Core/Sys/PropertyManager.cs)

One instance per property type per node, in a dictionary keyed by type id. `Add(typeId)` does nothing and returns null if the node already has that type; otherwise it creates the module, sets the owner, calls `SetDefaultValues`, and assigns its data in the module store (:51-66). `AddWithoutDefaults(instance)` skips the defaults (:39-49). `Remove(typeId)` drops the instance and its data (:68-75). `RemoveOrphanedProperties` is a self-described hack run at OK: it deletes module store instance data for **Color and Location only** (type ids `BFF34727-6B88-4F87-82B7-68424498C725`, `3FB53423-DD2A-4719-B3E3-19AA6F062F62`) whose instance id is not on any current **leaf** node (:99-118). So a Color or Location property on a group node loses its stored data at OK, and comes back with default values at the next load. Keep this: `commit` does the same.

### Element tree export file (`.v3e`)

JSON written by `ElementTreeWriter` (Vixen.Core/IO/JSON/ElementTreeWriter.cs): Newtonsoft, indented, camelCase names (`CustomContractResolver`), nulls and default values omitted, empty lists omitted. The root object is the invisible Root node, so shape is:

```json
{
  "id": "…root guid…",
  "name": "Root",
  "children": [
    { "id": "…", "name": "Arch", "children": [
      { "id": "…", "name": "Arch Px-1", "element": { "id": "…", "name": "Arch Px-1" } }
    ] }
  ]
}
```

Fields are from `ElementNodeProxy` and `ElementProxy` only: `id`, `name`, `element`, `children`. A multi-parent node appears once per parent. Written on a background task. Read back by `ElementTreeReader` for the sequence element mapper.

### Fixture profiles

`FixtureSpecificationManager.Save(fixture)` serialises the profile to an XML file in the fixture directory, named by `fixture.GetFileName()`, and updates its cached list (Vixen.Modules/App/FixtureSpecificationManager/FixtureSpecificationManager.cs:188-225). This happens when the wizard finishes, before Display Setup's OK, and Cancel does not revert it. Each fixture node's Intelligent Fixture property holds its own specification in module data.

### Controller run state on disk

The saved `disabled_devices` list (`SystemData.disabled_devices`) is inferred at save time: when the system is started, it is every device that is not running (`SaveDisabledDevices`, Vixen.Core/Sys/VixenSystem.cs:145-150, called at :177-180). So Stop / Disable on a controller persists through OK, and Cancel's reload restores the earlier list. A controller added while the system is started starts at once, since `Add` matches the execution state (OutputDeviceCollectionExecutionMediator.cs:89-106).

### Load and reload

`ReloadSystemConfig` closes execution, reruns `LoadSystemConfig`, and reopens execution if it was open (VixenSystem.cs:334-342). `LoadSystemConfig` builds new managers, loads the module store then the system config, adds elements, nodes, controllers, previews, filters, seeds any missing built-in tags, restores patches, and removes tag assignments that have no definition (:230-287, :308-332). `SaveSystemConfigAsync` copies the live collections into the config object and writes on a background task, ignoring a second request while one is running (:161-200). **Mapping to the Rust side:** `vixen_io::SystemData` already holds `channels` (elements), `tree`, `controllers`, `filters`, `tags`, `patches`, `disabled_devices`, `module_store`, and `save_system` writes it. `vixen_io::load_system` and `AppCore.system` are the entry points (crates/vixen-session/src/app.rs).

## Rust API needed

Signatures are guidance. Nothing here exists yet unless stated. Types are by value or borrow as suits; FFI records mirror them. Already available and to be reused: `vixen_core::{ElementTree, ElementNode, NodeId}` with `leaves`, `distinct_leaves`, `non_leaves`, `descendants`, `all_parents`, `max_child_depth`, `add_node`, `add_child`, `remove_child`, `property`; `vixen_io::{SystemData, Controller, ControllerOutput, OutputFilter, Patch, TagDefinition, load_system, save_system, save_module_store, ModuleStore}`; `vixen_output::{ids::{controller_descriptor, filter_descriptor}, filters::FilterConfig (with new_default, output_names, display_name), runner::ControllerRunner (start, stop, is_running, set_enabled), graph::OutputGraph}`; `vixen_session::app::AppCore`, `vixen_session::show::{graph_definition, build_output_graph}`. Controller setup, property setup, filter setup and the helper dialogs' APIs are in docs/spec/property-filter-controller-setup.md.

### vixen-session: tree edits (crates/vixen-session/src/setup/tree.rs)

These are the rules of the Data section. `vixen_core::ElementTree` is shared read-only with render threads and has no removal of nodes, so edits happen on a working copy with tombstones, and `ElementTree` is rebuilt from it at OK. This has been started as `SetupTree` in crates/vixen-session/src/setup/tree.rs, using `Nid` where the list below says `NodeId`, with `from_element_tree` and `to_element_tree`. Where that file's signatures differ from this list, the file wins; the rules here still apply.

```text
fn invalid_children(&self, node: NodeId) -> HashSet<NodeId>                // itself, direct children, all ancestors (visited set)
fn invalid_root_nodes(&self) -> HashSet<NodeId>                            // the current roots
fn uniquify_name(&self, name: &str, ignoring: Option<NodeId>) -> String    // "{name} - {n}" from 2; O(1) via a name count map kept in the tree
fn create_node(&mut self, parent: Option<NodeId>, name: Option<&str>, with_element: bool, uniquify: bool) -> NodeId   // CreateSingle; None name is "Unnamed"
fn add_child_to_parent(&mut self, child: NodeId, parent: Option<NodeId>, index: Option<usize>) -> TreeEffects      // converts a leaf parent to a group, reports the removed element
fn remove_node(&mut self, node: NodeId, parent: Option<NodeId>, cleanup: bool) -> TreeEffects                     // emptied group gets a new element; cleanup cascades through orphans
fn move_node(&mut self, node: NodeId, new_parent: Option<NodeId>, old_parent: Option<NodeId>, index: Option<usize>) -> TreeEffects   // same-parent index adjust, no throwaway element
fn rename_node(&mut self, node: NodeId, name: &str)                        // no-op when equal; unique except against itself
fn sort_children(&mut self, group: NodeId)                                 // F51 comparison: case-insensitive first, then lower before upper
fn reverse_selected(&mut self, rows: &[RowPath]) -> Result<(), NotContiguous>   // F50, F72
fn can_reverse(rows: &[RowPath]) -> bool                                   // same parent, consecutive indexes
fn duplicate_subtree(&mut self, node: NodeId, parent: Option<NodeId>) -> NodeId  // F37 including its property-copy gap on child groups, see note below
fn delete_row_set(&mut self, rows: &[RowPath]) -> TreeEffects              // F44: in order, skipping a row whose parent link has already gone; never creates an element for a dead node
struct TreeEffects { removed_elements: Vec<Uuid>, added_elements: Vec<Uuid>, removed_nodes: Vec<NodeId> }   // the session applies these to data flow and module data
type RowPath = Vec<NodeId>                                                 // identity of a tree row
fn first_parent_path(&self, node: NodeId) -> RowPath                       // F27
fn drop_target(&self, target: &RowPath, position: DropPosition, target_is_expanded: bool) -> DropResolution  // F57: { new_parent, index, expand }
fn can_drop(&self, sources: &[NodeId], resolution: &DropResolution, copying: bool) -> bool   // F59
fn can_paste(&self, clipboard: &[NodeId], destination: Option<NodeId>, as_new: bool) -> bool  // F36
```

Note on `duplicate_subtree`: copying properties on child groups is a one-line improvement, but it changes results; keep upstream's behaviour in Rust, with the gap marked in a comment and a test, unless the owner asks otherwise.

### vixen-core: naming (new crates/vixen-core/src/naming.rs)

```text
enum NamingRule { Numeric { start: i32, end: i32, endless: bool, step: i32 }, Letters { start: char, count: i32 }, LetterList(String), WordList(Vec<String>) }
impl NamingRule { fn iterations(&self) -> i32; fn is_endless(&self) -> bool; fn name_at(&self, i: i32) -> String; fn display_name(&self) -> &str }  // F136
fn generate_names(format: &str, rules: &[NamingRule], wanted: usize) -> Vec<String>       // F135
fn replace_patterns(old: &[String], patterns: &[(String, String)]) -> Vec<String>         // F137
fn numbered_items_template(seed: &str) -> (String, Vec<NamingRule>)                       // F134
fn uniquify_in(names: &HashSet<String>, name: &str) -> String                             // NamingUtilities.Uniquify
```

### vixen-output: mutable data flow graph (new crates/vixen-output/src/dataflow.rs)

`OutputGraph` is the compiled, read-only form used at runtime. Display Setup needs the editable form, the port of `DataFlowManager`, from which an `OutputGraph` is rebuilt when the window closes with OK.

```text
enum ComponentId { Element(Uuid), Filter(Uuid), Output(Uuid) }             // filters by instance id, outputs by CommandOutput id
struct SourceRef { component: ComponentId, output: i32 }
struct DataFlow { sources: HashMap<ComponentId, SourceRef>, destinations: HashMap<ComponentId, Vec<ComponentId>>, output_counts: HashMap<ComponentId, u32> }   // destinations stay in connection order
fn from_system(system: &SystemData, store: &ModuleStore) -> DataFlow       // ignores patches with a missing end or bad index, reporting each
fn to_patches(&self) -> Vec<Patch>                                         // one per component with a source; keeps destination order, which patch point order depends on
fn set_source(&mut self, c: ComponentId, s: SourceRef) -> bool             // false if it would form a cycle
fn reset_source(&mut self, c: ComponentId)
fn add_component(&mut self, c: ComponentId, output_count: u32)
fn remove_component(&mut self, c: ComponentId)                             // detaches both directions
fn set_output_count(&mut self, c: ComponentId, n: u32)                     // after a filter setup; drops destinations on removed outputs
fn destinations(&self, c: ComponentId) -> &[ComponentId]
fn destinations_of_output(&self, c: ComponentId, output: i32) -> impl Iterator<Item = ComponentId>
fn would_cycle(&self, component: ComponentId, source: ComponentId) -> bool
fn root_source(&self, c: ComponentId) -> ComponentId                       // F84 upward walk, iterative
fn patch_points(&self, c: ComponentId) -> Vec<PatchPoint>                  // F61; memoizable
struct PatchPoint { component: ComponentId, output: i32, patched: bool }
fn reachable_filters(&self, c: ComponentId) -> Vec<Uuid>                   // F62
fn orphaned_filters(&self) -> Vec<Uuid>                                    // F6: filters with no source, and chains under them
```

### vixen-session: Display Setup session (crates/vixen-session/src/setup/, next to `tree.rs`)

The session is a **working copy** of the system (`SystemData` clone plus `DataFlow`), so Cancel is dropping it. All methods are safe to call off the main thread; the FFI wraps the session in a lock and returns plain records.

```text
struct DisplaySetupSession { /* working SystemData, DataFlow, name counts, clipboard state, revision counters */ }
fn open(core: &AppCore) -> DisplaySetupSession
fn commit(self, core: &AppCore, progress: impl Fn(u32, &str)) -> Result<(), SessionError>   // F6: orphans, properties, swap into AppCore, save both files, rebuild the output graph
fn discard(self, core: &AppCore)                                                            // F7: restore controller run states changed during the session
fn is_dirty(&self) -> bool                                                                  // for the Mac close confirmation
fn tree_revision(&self) -> u64; fn patching_revision(&self) -> u64

// tree rows, fetched in bulk per parent and cached in Swift
struct ElementRow { path: String, name: String, icon: RowIcon, has_children: bool, tag_colors: Vec<u32> }
enum RowIcon { Group, Patched, PatchedMasked, Unpatched }                                   // F22
fn element_rows(&self, parent_path: Option<&str>, start: u32, count: u32) -> (Vec<ElementRow>, u32 /*total*/)
fn element_icons(&self, paths: &[String]) -> Vec<RowIcon>                                   // refresh visible rows only

// element commands (each returns what changed so Swift reloads only those parents)
fn add_element(&mut self, parent: Option<&str>, name: &str) -> EditResult                   // F40
fn add_elements(&mut self, parent: Option<&str>, names: &[String]) -> EditResult            // F41
fn delete_rows(&mut self, paths: &[String]) -> EditResult                                   // F12, F28, F42, F44; one result per gesture
fn create_group(&mut self, paths: &[String], name: &str) -> EditResult                      // F45
fn rename_rows(&mut self, paths: &[String], names: &[String]) -> EditResult                 // F47, F49
fn paste_links(&mut self, destination: Option<&str>) -> EditResult;  fn paste_as_new(&mut self, destination: Option<&str>) -> EditResult   // F35, F37; skip clipboard ids deleted since the copy
fn copy_rows(&mut self, paths: &[String]);  fn clipboard_state(&self, destination: Option<&str>) -> (bool, bool)  // F34, F36
fn would_lose_patches(&self, destination: Option<&str>) -> bool                             // F43
fn drop_rows(&mut self, sources: &[String], target: &str, position: DropPosition, target_expanded: bool, copy: bool) -> EditResult   // F57, F58
fn can_drop_rows(&self, sources: &[String], target: &str, position: DropPosition, target_expanded: bool, copy: bool) -> bool   // cached per hovered row; F59
fn sort_group(&mut self, path: &str);  fn reverse_rows(&mut self, paths: &[String]) -> EditResult
fn context_menu_state(&self, selection: &[String]) -> TreeMenuState                         // every F32 rule, so the table is testable

// tags
fn tag_menu(&self, selection: &[String]) -> Vec<TagMenuItem>                                // id, name, colour, tri-state; F54
fn toggle_tag(&mut self, selection: &[String], tag: Uuid, cascade: bool)                    // F55
fn tag_colors(&self) -> Vec<TagColorRow>;  fn set_tag_colors(&mut self, colors: &[(Uuid, String)])  // F132

// properties
fn properties_of(&self, primary: &str) -> Vec<PropertyRow>                                  // type id, TypeName, in order
fn add_property(&mut self, selection: &[String], type_id: Uuid)                             // F17
fn remove_properties(&mut self, selection: &[String], type_ids: &[Uuid])                    // F18
fn clone_property_to_selection(&mut self, selection: &[String], source: &str, type_id: Uuid)   // F19 branch 2
fn copy_properties(&mut self, path: &str);  fn paste_properties(&mut self, selection: &[String])   // F38, F39

// simple patching
struct ElementStats { total: u32, connected: u32, unconnected: u32, items: u32, groups: u32, leaves: u32, filters: u32 }
struct ControllerStats { outputs: u32, patched: u32, unpatched: u32, controllers: u32, first_label: String, last_label: String }
struct PatchOptions { all_points: bool, all_outputs: bool, reverse_elements: bool, reverse_outputs: bool }
struct PatchSummary { sources: u32, destinations: u32, text: String, warning: String, can_patch: bool }
enum OrderCheck { TreeOrder, UseOrder, MissingSome, Duplicates }                            // F69 step 2
fn element_stats(&self, selection_in_order: &[String]) -> ElementStats                      // F62, off main with generation tokens
fn controller_stats(&self, selection: &OutputSelection) -> ControllerStats                  // F65
fn patch_summary(&self, stats: &ElementStats, controllers: &ControllerStats, opts: &PatchOptions) -> PatchSummary   // F68
fn order_check(&self, selection_in_order: &[String]) -> OrderCheck
fn patch_elements(&mut self, selection_in_order: &[String], outputs: &OutputSelection, opts: &PatchOptions, use_order: bool) -> u32   // F69, returns the count
fn unpatch_elements_preview(&self, selection: &[String]) -> (u32, bool)                     // patched count, any filters; F63
fn unpatch_elements(&mut self, selection: &[String], remove_filters: bool)
fn unpatch_outputs(&mut self, outputs: &OutputSelection)                                    // F66, F96, F126
fn find_outputs(&self, selection: &[String]) -> OutputSelection                             // F14
fn find_elements(&self, outputs: &OutputSelection) -> Vec<String>                           // F84

// controllers. OutputSelection holds ranges, not hash sets, so a whole 170,000-output controller is one entry
struct OutputSelection { items: Vec<(Uuid, Vec<Range<u32>>)> }                              // pane order
struct ControllerRow { id: Uuid, name: String, running: bool, output_count: u32, type_name: String }
enum OutputTreeRow { Output { index: u32, name: String, icon: OutputIcon }, Page { start: u32, count: u32, title: String } }   // F80, 5000 per page
enum OutputIcon { Unpatched, BrokenSource, Patched }
fn controller_rows(&self) -> Vec<ControllerRow>
fn output_rows(&self, controller: Uuid, page_start: Option<u32>) -> Vec<OutputTreeRow>
fn add_controller(&mut self, type_id: Uuid, name: &str, outputs: u32) -> Result<Uuid, String>  // F77
fn output_limit(&self, type_id: Uuid) -> u32
fn rename_controller(&mut self, id: Uuid, name: &str);  fn delete_controllers(&mut self, ids: &[Uuid])   // F89, F90
fn set_output_count(&mut self, id: Uuid, count: u32)                                        // F88
fn insert_outputs(&mut self, id: Uuid, at: u32, count: u32)                                 // F94; off main
fn remove_outputs(&mut self, id: Uuid, indexes: &[u32])                                     // F95; off main
fn move_controllers(&mut self, ids: &[Uuid], up: bool)                                      // F78, applied immediately
fn reorder_controllers(&mut self, order: &[Uuid])                                           // F79
fn start_controllers(&mut self, ids: &[Uuid]);  fn stop_controllers(&mut self, ids: &[Uuid])   // F91, through ControllerRunner
fn controller_menu_state(&self, selection: &ControllerTreeSelection) -> ControllerMenuState  // F86, F92, F93 rules, wording included

// graphical view
struct GraphScene { shapes: Vec<GraphShape>, lines: Vec<GraphLine>, content_size: (f32, f32) }
struct GraphShape { id: u64, kind: ShapeKind, title: String, rect: Rect, depth: u8, inputs: Vec<(f32, f32)>, outputs: Vec<(f32, f32)>, hidden: bool, source: ComponentId }
struct GraphLine { from_shape: u64, from_port: u32, to_shape: Option<u64>, to_port: u32, stub: StubKind }   // a hidden child's line ends at its collapsed group's edge midpoint, F124
enum ShapeKind { ElementGroup, ElementLeaf, Filter, Controller, Output }
fn graph_scene(&self, elements: &[String], outputs: &OutputSelection, view: GraphViewState, width: f32) -> GraphScene   // F112, F115, F116; off main
struct GraphViewState { collapsed: HashSet<u64>, filter_positions: HashMap<Uuid, (f32, f32)>, floating_filters: Vec<Uuid> }   // filter depth is not stored: computed at each layout, F115
fn graph_connect_candidate(&self, source: ComponentId, target: ComponentId) -> bool          // F119 refusal rules
fn graph_connect(&mut self, target: ComponentId, source: SourceRef)                          // F119
fn graph_delete(&mut self, lines: &[ComponentId] /*destinations; a right stub has none, so it is never passed*/, filters: &[Uuid])   // F120; lines and filters as one set, one PatchingUpdated
fn graph_add_filter(&mut self, type_id: Uuid) -> Uuid                                        // F121
fn graph_paste_filters(&mut self, sources: &[Uuid], copies: u32) -> Vec<Uuid>                // F122, F123
```

### vixen-session: element templates (new crates/vixen-session/src/element_templates.rs)

Used by Display Setup and by Preview (`vixen_preview::Tool::element_template` names the same templates). Pure over `ElementTree`.

```text
enum TemplateKind { SingleItem, NumberedGroup, Arch, CandyCane, Star, Starburst, Icicles, LipSync, Megatree, PixelGrid, IntelligentFixture }
fn all_templates() -> Vec<(TemplateKind, &'static str)>                                     // F99, in display order
fn template_by_name(name: &str) -> Option<TemplateKind>
enum StartLocation { BottomLeft, BottomRight, TopLeft, TopRight }
enum TemplateParams {
  Single { name: String },
  Numbered { group: String, prefix: String, count: u32 },
  Starburst { name: String, spokes: u32, spoke_prefix: String, pixel: bool, pixels: u32, pixel_prefix: String },
  Icicles { name: String, strings: u32, pattern: String, string_prefix: String, pixel_prefix: String },
  LipSync { name: String },
  Megatree { name: String, strings: u32, string_prefix: String, pixel: bool, pixels: u32, pixel_prefix: String, start: StartLocation, zig_zag: bool },
  PixelGrid { name: String, horizontal: bool, rows: u32, columns: u32, string_prefix: String, pixel_prefix: String, start: StartLocation, zig_zag: bool },
}
fn template_defaults(kind: TemplateKind) -> TemplateParams
fn validate_template(params: &TemplateParams) -> Result<(), TemplateError>                  // F105's message, and empty names
struct TemplateResult { head: NodeId, created: Vec<NodeId>, leaves: Vec<NodeId> }
fn generate_template(tree: &mut ElementTree, params: &TemplateParams) -> Result<TemplateResult, TemplateError>   // one batch, no per-node events
fn add_patching_order(tree: &mut ElementTree, nodes: &[NodeId]);  fn add_patching_order_zigzag(tree: &mut ElementTree, nodes: &[NodeId], every: u32)   // F109
fn zig_zag_every(params: &TemplateParams) -> u32                                            // the read-only number
fn template_needs_followups(kind: TemplateKind) -> (bool /*dimming*/, bool /*color*/)       // F100
```

### vixen-session: Intelligent Fixture template (new crates/vixen-session/src/fixture_template.rs)

```text
struct FixtureWizardChoices { profile: FixtureSpecification, color_support: ColorSupport, auto_shutter: bool, auto_color_wheel: bool, auto_dimmer: bool, auto_prism: bool, dimming: DimmingCurveSelection, curve: Curve, count: u32, prefix: String, create_group: bool, group_name: String }
enum ColorSupport { None, Mixing, Wheel }
enum DimmingCurveSelection { NoDimmingCurve, OneDimmingCurvePerFixture, OneDimmingCurvePerColor }
fn default_color_support(profile: &FixtureSpecification) -> ColorSupport                    // F143
fn automation_defaults(profile: &FixtureSpecification, support: ColorSupport) -> AutomationState   // F144: enabled and default for each box
fn wizard_summary(choices: &FixtureWizardChoices) -> Vec<(String, String)>                  // F147
fn create_fixture_elements(session: &mut DisplaySetupSession, choices: &FixtureWizardChoices) -> Result<FixtureResult, String>   // F110 in full, in channel order
fn wizard_preview_names(prefix: &str, count: u32, group: Option<&str>) -> Vec<String>       // F146
```

`FixtureSpecification`, its load and save, and `SupportsFunction` belong to the fixture crate that the fixture editor's spec defines. If it does not exist when this area is built, implement only those parts here, in `vixen-io`, reading and writing the same XML.

### vixen-export: wire diagram (new crates/vixen-export/src/wire_diagram.rs)

```text
struct WireDiagramOptions { label_interval: u32, node_radius: f32, label_font_size: f32, export_scale: f32 }   // default 1, 6, 14, 1
fn validate(&self) -> Result<(), String>                                                     // ranges of F128
fn wire_diagram_points(tree: &ElementTree, group: NodeId) -> Result<Vec<((i32, i32), i32)>, NoLocation>   // F129
fn wire_diagram_svg(points: &[((i32, i32), i32)], flip: bool, options: &WireDiagramOptions) -> Option<String>   // F130
```

### vixen-io: tree export (crates/vixen-io)

```text
fn write_element_tree(tree: &ElementTree, root_id: Uuid, path: &Path) -> Result<(), IoError>   // F53, the JSON above
fn read_element_tree(path: &Path) -> Result<ElementNodeProxy, IoError>                         // used by the sequence mapper
```

### vixen-ffi (crates/vixen-ffi/src/setup.rs)

The `setup` feature exposes `DisplaySetupSession` as a `uniffi::Object` with the methods above. Heavy calls are `async` or run on a background executor: `commit`, `graph_scene`, `element_stats`, `generate_template`, `create_fixture_elements`, `insert_outputs`, `remove_outputs`. Paths and ids cross the boundary as strings. Selections cross as arrays in selection order. No per-row, per-draw calls: Swift caches `ElementRow`s by parent and invalidates by the `EditResult`.

## SwiftUI and AppKit structure

### Scene and module

- Module `app/Sources/VixenSetup`, importing VixenKit and VixenBridge, exporting `SetupScene` with `init(model: AppModel)`, the name app/Sources/VixenApp/VixenApp.swift and app/Package.swift already look for. It may also import `VixenDialogs` (the one extra dependency Package.swift allows it), which holds the property, filter and controller dialogs. FFI in `crates/vixen-ffi/src/setup.rs` under the `setup` feature, which exists and is empty.
- Scene: `Window("Display Setup", id: WindowID.displaySetup)`, one instance (`"display-setup"`, the id the test harness example in docs/UI_GUIDE.md already uses), `.defaultSize(width: 1281, height: 737)`, resizable, sensible minimum (about 1100 x 640). Opened from the main window's Setup Display button and the menu item. It is modal in upstream, so while it is open the main window disables its Setup buttons and any command that reads or writes the system model, through a flag on the app model.
- State: `@Observable final class DisplaySetupModel`, holding the Rust session, the element selection in selection order, the logical controller selection, the current patching view, per-template drafts, revision counters. Created on open, dropped on close. No singletons.
- `docs/parity/display-setup.md` lists every F entry and every upstream control.
- Closing: the OK flow runs `commit` with a progress display on the main window, as upstream does (F6). Cancel, Esc and the close box run `discard` (F7).

### Window layout

```
DisplaySetupRoot (SwiftUI)
├ HSplitView with 25% / 50% / 25% initial widths (upstream is a fixed table with no splitters; the splitters are the small Apple-ism)
│ ├ ElementsPanel (VStack)
│ │ ├ header "Elements", Divider
│ │ ├ Add row: "Add:", Picker(.menu), + button
│ │ ├ ElementOutline (NSViewRepresentable over VixenKit.OutlineView, fills)
│ │ ├ button row: Delete, Rename, Find outputs
│ │ └ GroupBox "Selected Item(s):" Configure row, Properties list and its 3 buttons
│ ├ PatchingPanel (VStack)
│ │ ├ header "Patching", Divider
│ │ ├ two radio choices side by side, as Picker(.radioGroup) horizontal: "Simple Patching", "Graphical View"
│ │ └ SimplePatchingView | GraphicalPatchingView (keep both alive; switch by opacity or hold state in the model)
│ └ ControllersPanel (VStack)
│   ├ header "Controllers", Divider
│   ├ Add row: "Add:", Picker, +, up, down
│   ├ ControllerOutline (OutlineView, fills)
│   ├ button row: Find elements
│   ├ GroupBox "Selected Controller": Type, Outputs, 6 buttons
│   └ OK, Cancel, Help, right aligned at the bottom
```

### What must be AppKit

- **Both trees: `NSOutlineView`**, via `VixenKit.OutlineView` (app/Sources/VixenKit/OutlineView.swift). It needs extending, in VixenKit so others benefit:
  - row identity is a path string (`guid/guid/guid`), since a node can appear under several parents; the provider protocol already works in strings;
  - selection in order: report the ordered list and the anchor, as well as the `Set`, tracking ⇧ gestures (F71, F26). Patching order depends on this;
  - a row image kind (folder, or a coloured ball via the existing `statusColor`) and optional trailing tag dots (F25);
  - drops: report above/on/below, target path and whether the target is expanded (F57), with the ⌥ copy state, and a validate call cached per hovered row;
  - programmatic selection with reveal, and scroll a row to top (F27);
  - expansion restored by path across reloads, reloading only the parents an edit touched;
  - children fetched in ranges for a huge group, rather than the current whole-array `outlineChildren(of:)`, or at least cached per parent off the main thread.
- **Graphical canvas: one custom `NSView` (`GraphCanvasView`)** with layer-backed drawing, wrapped in `NSViewRepresentable` inside an `NSScrollView` (magnification for pinch zoom). Shapes are not views. Draw only shapes that intersect the dirty rect, from the `GraphScene` Rust computes; keep the in-progress connection line and the selection frame on a separate `CAShapeLayer`, so dragging never redraws the graph. Port dots, grids and gradients are cheap fills. A title's text layout is cached per shape at the current width. Below a zoom threshold, skip the text and the dots.
- Everything else is SwiftUI: small `Grid`s, `GroupBox`es, sheets, the properties `List` (few rows), the wizard.

### Control mapping

Mac forms keep each control's region and order. Tooltips become `.help` with the original text. SF Symbols replace Vixen's stock icons where the meaning is the same.

| Original control | Mac form |
|---|---|
| `comboBoxNewItemType`, `comboBoxSetupHelperType`, `comboBoxNewControllerType` | `Picker` with `.menu` style |
| `buttonAddTemplate`, `buttonAddProperty`, `buttonAddController`, `buttonAddFilter` | icon button `plus` |
| `buttonRunHelperSetup` | `gearshape.2`, `.help("Configure")` |
| `buttonConfigureProperty`, `buttonConfigureController` | `gearshape` |
| `buttonRemoveProperty`, `buttonDeleteElements`, `buttonDeleteController`, `buttonDeleteFilter` | `minus` or `trash`, destructive role on confirmation |
| `buttonRenameElements`, `buttonRenameController` | `pencil` |
| `buttonSelectDestinationOutputs`, `buttonSelectSourceElements` | `text.magnifyingglass` |
| `buttonRaiseControllerOrdering`, `buttonLowerControllerOrdering` | `arrow.up`, `arrow.down` |
| `buttonNumberChannelsController` | `number`, `.help("Channel Count")` |
| `buttonStartController`, `buttonStopController` | `play.fill`, `stop.fill` |
| `buttonZoomIn`, `buttonZoomOut`, `buttonZoomFit` | `plus.magnifyingglass`, `minus.magnifyingglass`, `arrow.up.left.and.down.right.magnifyingglass` |
| `buttonHelp` | Button "Help" with `questionmark.circle`, opening the URL `https://www.vixenlights.com/docs/usage/display-setup/` |
| `radioButtonPatching*`, `radioButton*Output*`, `radioButton*PatchPoints*` | `Picker(.radioGroup)` |
| `checkBoxReverse*` | `Toggle(.checkbox)` |
| Count labels | `LabeledContent` in a `Grid`, `.monospacedDigit()`, bold where upstream is bold |
| `labelPatchWarning` | Yellow text, keeping the original wording |
| `listViewProperties` | Small `List(selection:)` with no header |
| Tree context menus | `NSMenu` built from Rust's menu state (`TreeMenuState`, `ControllerMenuState`) |
| `MessageBoxForm` prompts | `.alert` or `.confirmationDialog` with upstream's title and text, and verbs on buttons: F12 "Delete", "Cancel"; F18 "Remove", "Cancel"; F43 "Convert to Group", "Cancel"; F63 "Unpatch", "Cancel" for the count prompt and "Remove Filters", "Keep Filters", "Cancel" for the filter one; F69 "Open Order Wizard", "Use Tree Order", "Cancel"; F90 "Delete", "Cancel". Destructive buttons use `role: .destructive`. |
| `TextDialog`, `NumberDialog`, `ListSelectDialog` | Sheets on the Display Setup window (F148) |
| Template dialogs, `NameGenerator`, `SubstitutionRenamer`, Manage Tag Colors, Wire Diagram options | Sheets on the Display Setup window, `Form`/`Grid` matching the upstream field order |
| Intelligent Fixture Wizard | Its own `Window` with a step sidebar, since it hosts large editors |
| `SaveFileDialog` (F53) | `NSSavePanel`, or `.fileExporter`, type `.v3e` |
| Progress during OK and Cancel | On the main window's existing progress area, same texts |

Enable state: compute in Rust where it depends on data (`TreeMenuState`, `ControllerMenuState`, stats), and bind buttons to the model. Never reach across FFI inside a `body` for each button.

### Refresh policy

Selection changes write to the model at once and update the buttons at once from local state. Stats and graph scene computations are asynchronous, tagged with a generation number, and stale answers are dropped, so a held ⇧↓ never queues work. The patching panel shows the last good numbers until the new ones arrive.

### Testing hooks

Icon buttons get `accessibilityLabel` equal to their upstream tooltip, so `{ "action": "click", "label": "Add Elements" }` works in the harness. The harness steps in docs/UI_GUIDE.md run against `window` id `display-setup`. Tests must run on a copy of `SystemData` under `target/`, never the owner's real show, and controllers stay stopped.

## Performance hazards

The owner named this window as slow and choppy. Upstream's own measurements are in upstream/Vixen/docs/plans/display-setup/display-setup-ok-performance.md: a profile of 27 controllers and 163,655 outputs spent about 33 seconds in native `TreeView` item deletion when the window closed, before they added paging. Those are the conditions to design for, on top of the 50,000-row rule in docs/UI_GUIDE.md.

- **Native tree items.** Upstream creates a `TreeNode` per visible row and pays heavily on creation and teardown (ElementTree.cs:316-342; ControllerTree.cs:376-417). Do instead: `NSOutlineView` with lazily fetched rows. No placeholder rows, no eviction logic. Keep 5,000-per-page output rows for navigation only.
- **Every click recomputes everything.** A selection change walks the entire selected subtree and all its data flow (SetupPatchingSimple.cs:121-143, :249-295). The filter count nests LINQ `SelectMany` and `Concat` once per data flow level, per node, with no sharing (:308-315), and patch points recurse per output (:317-356). With a group of tens of thousands of leaves selected, each click or ⇧-arrow costs seconds on the UI thread. Do instead: run stats in Rust off the main thread, with a generation token. Hold per-leaf results (patch point counts, filter counts) in a table invalidated by patching revision, so a selection's stats are a sum over distinct leaves. Coalesce to one computation per display refresh.
- **Selection bookkeeping.** `m_SelectedNodes.Contains` is linear, so selecting n rows is quadratic (MultiSelectTreeview.cs:245-250, :1068-1076). `TreeNodeSorter` walks the whole tree per comparison when parents differ (:1370-1395), and `_sortSelectedNodesWhenUpdateEnds` is never cleared, so every later `EndUpdate` sorts again (:302-309). Do instead: `NSOutlineView`'s index set, with the anchor and direction kept by the wrapper. Convert to paths only when something needs them, and only send the selection across FFI after the gesture settles or per display refresh.
- **Repopulate on each edit.** Most commands clear and rebuild the tree, then re-expand and re-select by string path, each lookup scanning siblings (ElementTree.cs:125-193, :257-283). Do instead: Rust returns which parents changed, and Swift calls `reloadItem(_:reloadChildren:)` for only those, keeping expansion and selection natively.
- **One event per row, several per edit.** Deleting n rows raises ElementsChanged n times before the final repopulate, and each one recomputes the patching view, refreshes every materialized controller output and, in the Graphical view, rebuilds the scene and refits the zoom (ElementTree.cs:739; SetupElementsTree.cs:361-365; DisplaySetup.cs:82-97). A plain Add raises it up to three times (ElementTree.cs:809, :852, :1378). Do instead: one notification per user gesture, carrying what changed.
- **Add Multiple rebuilds the parent per row.** For each new node under a parent, `AddNodePathToTree` clears and re-adds all of that parent's children and searches them by path, which is quadratic in the group's size, up to the dialog's limit of 1,000,000 (ElementTree.cs:820-837). Do instead: `add_elements` is one batch, and Swift reloads the parent once.
- **Status icon refresh.** `RefreshElementTreeStatus` and `RefreshControllerOutputStatus` touch every materialized row on every patch change, doing a data flow lookup each (ElementTree.cs:360-405; ControllerTree.cs:631-650). Do instead: bump a revision, refetch icons for visible rows only (`element_icons(paths)`), in bulk.
- **Per-row draw work.** Each tree row paint allocates a tag list, does a linear tag lookup for each of its tags, parses an HTML colour and allocates brushes (ElementTree.cs:411-467). Do instead: resolve `tag_colors` once when fetching the row, with the row cache dropped when tags change.
- **Name uniquifying.** `_Uniquify` scans all nodes per call, and again per suffix attempt (NodeManager.cs:123-135). Adding n elements by `CreateSingle` with a uniquified name is quadratic, including Add Multiple, Single Item, LipSync, fixture nodes and duplication. Do instead: a name count map in `ElementTree`, updated on create, rename and removal.
- **Template generation.** Each node goes through `AddNode`, `ElementNode.Changed` (a static event with handlers), a throwaway `Element` for every future group, removed again when its first child arrives, and `DataFlow.AddComponent`, all on the UI thread (ElementNodeService.cs:20-35; NodeManager.cs:103-121; ElementNode.cs:172-182). A 1000 x 1000 grid is a million of those. Do instead: `generate_template` builds the shape in one batch off the main thread with no per-node notifications, creating elements only for final leaves, with a progress indicator for big counts, and one reload at the end.
- **Ancestor walks during a drag.** `treeviewDragVerifyHandler` calls `InvalidChildren` and LINQ `Intersect` on every `OnDragOver` (ElementTree.cs:656-719). `GetAllParentNodes` has no visited set, so a diamond-shaped hierarchy repeats itself exponentially (ElementNode.cs:288-291). Do instead: answer `can_drop_rows` once per hovered row and position, cached for the drag, with ancestors computed by a visited-set walk.
- **Whole-controller selection.** Selecting a controller materializes a `HashSet` of every output index, several times per click (ControllerTree.cs:694-720; SetupControllersSimple.cs:89,117-125). Then `_updateControllerDetails` looks up each output's adapter one by one (SetupPatchingSimple.cs:376-397), and `CheckIfSelectedControllersRunning` scans outputs for a patch (ControllerTree.cs:1258-1263). Do instead: `OutputSelection` as ranges, and per-controller patched counts maintained incrementally. First and last labels come from range ends without iterating.
- **Output list edits.** `RemoveOutputs` removes one at a time with linear list work, then re-indexes all (OutputController.cs:338-351). `InsertOutputsAt` removes and re-adds everything from the insertion point on (:268-298). `OutputCount` adds one output at a time with three dictionary inserts (:233-249, :315-322). Do instead: batch these in Rust against a `Vec`, off the main thread with a busy indicator.
- **OK and Cancel.** `Reorder` removes and re-adds every controller, which stops and restarts devices (OutputControllerManager.cs:219-244; mediator :89-122), so a reorder blips hardware at save time. `ApplyConfirmedChanges` runs on the UI thread, and Cancel reloads the entire configuration on the UI thread (VixenApplication.cs:998-1016). Do instead: controller order is a `Vec` reorder with no restarts. Cancel drops the working copy, on a background queue for huge shows, restarting only controllers whose run state was changed. OK's work runs off the main thread with progress.
- **Graphical view: one heavyweight object per thing.** NShape creates a shape, a repository entry and several caches per node, per output and per line, each with GDI+ gradients, in anti-aliased "high quality" mode (SetupPatchingGraphical.cs:126, :296-325, :1500-1522). Selecting a big group builds thousands of them on the UI thread, then fits the zoom. `ZoomToFitAll` and connection rebuilding run on every element and controller selection change (:2022-2070). `_allNodeParentsAreInSet` converts to a list on every recursion, and does linear `Contains`, giving quadratic behaviour or worse (:268-273). `UpdateConnections` loops all shapes with LINQ per port (:1735-1855). This view is the worst offender and the most likely source of the owner's memory. Do instead: `graph_scene` in Rust produces flat arrays off the main thread. Hash sets replace list scans. Selection changes diff scenes by shape id, so a small change does not rebuild anything visible.
- **Graphical view: paint and mouse.** `_DrawTitle` calls `MeasureString` per shape per paint and allocates a new `Font` while shrinking (FilterSetupShapeBase.cs:133-166). `ConnectionTool.Draw` loops over every shape on every paint to draw ports (ConnectionTool.cs:143-157), and a plain mouse move invalidates (`ProcessMouseMove` with `Action.None`, :345-350). Moving a filter clones preview shapes and their connected lines, and resets them from the originals on every move (:863-887, :1322-1358). Do instead: draw only the dirty rect. Use a uniform grid for hit tests, built with the scene. Cache a title's layout and only redo it when width changes. Hover needs no redraw unless the highlighted port changes. During a filter drag, translate the one filter's layer and its attached lines on the overlay layer until mouse up.
- **Graphical view: everything goes through NShape's history.** Each shape and line a rebuild creates or deletes is a command kept for undo (DiagramSetController.cs:221-257; Project.cs:428-435), and `InsertShape` also changes the selection and sends notifications per shape (Display.cs:1869-1876), which the view then has to undo by reselecting (SetupPatchingGraphical.cs:2026, :2040). So memory grows with every selection change for as long as the window is open. Do instead: a scene is plain data with no history; the only undo entries are filter moves.
- **Zoom.** Upstream re-renders the whole diagram at each zoom step. Do instead: `NSScrollView` magnification with a scaled redraw on gesture end. Skip titles and ports at very small sizes.
- **FFI.** Never call across per row, per draw. Fetch rows in batches by parent, cache in Swift, and keep the session's lock uncontended by running heavy commands on a background executor and showing their results on completion.

## Tests worth writing

Unit tests in Rust, without a window. Each list item gives inputs and expected values read from the code.

### Upstream tests covering this area (Vixen.Tests)

- `Common/ControllerTreeVirtualizationTests.cs` 400: paging, eviction, logical selection, including a logical selection replacing the previous outputs and clearing their highlights, a rebuild keeping the top row without selecting it, identity surviving duplicate output names, and an ordinary click or an empty-space click working after a programmatic selection. Port the behaviours for `output_rows` and selection; the eviction tests no longer apply. Pinned values: 1, 4999, 5000 outputs give direct leaves; 5001 gives 2 pages, last "Outputs 5001-5001"; 10000 gives 2, last "Outputs 5001-10000"; 15000 gives 3, last "Outputs 10001-15000".
- `Common/MultiSelectTreeviewKeyboardSelectionTests.cs` 197 and `Common/MultiSelectTreeviewRangeSelectionOrderTests.cs` 125: ⇧ ranges, anchor-to-endpoint order, shrinking and crossing the anchor, collapsed descendants skipped, Ctrl+Down does nothing, Delete not consumed by the shared control. Port as tests of the Swift wrapper's selection model, or move that logic to Rust so they can run there.
- `Setup/SetupPatchingSimpleOutputOrderTests.cs` 342: first and last labels, destination order. Pinned: `Main` selection {1700, 1638, 1703, 1650} of 1704 outputs gives labels "Main #1639" and "Main #1704", destinations 1638, 1650, 1700, 1703, and reversed 1703, 1700, 1650, 1638 with labels unchanged. `First` {5, 1} and `Second` {3, 0} give "First #2", "Second #4", destinations (First 1, First 5, Second 0, Second 3), reversed (Second 3, Second 0, First 5, First 1). `Paged` 5001 outputs {5000, 4999, 0} gives "Paged #1", "Paged #5001", destinations 0, 4999, 5000.
- `Setup/SetupPatchingSimpleElementOrderTests.cs` 80: A, B, C with C selected then ⇧-click A yields selection order C, B, A through to patching.
- `Setup/OrderSetupHelperZigZagTests.cs` 202: the order helper's zig zag (specified in the helper's spec).
- `Core/OutputControllerOutputIndexTests.cs` 275: lookups after `RemoveOutputs` and `InsertOutputsAt`.
- `Services/ElementTagServiceTests.cs` 94, `Sys/BuiltInElementTagsTests.cs` 70, `Sys/ElementTagCollectionTests.cs` 172, `Sys/ElementTagDefinitionTests.cs` 40: tags.
- `Utility/NamingUtilitiesTests.cs` 67: `foo` not in set stays; in set gives `foo - 2`; with `foo - 2` present gives `foo - 3`.
- `IO/Xml/Serializer/XmlElementTagDefinitionSerializerTests.cs` 111, `IO/Xml/Serializer/XmlElementTagDefinitionCollectionSerializerTests.cs` 55: tag definitions in the system config (crates/vixen-io's side).
- `Setup/SetupPatchingTestOutputModuleConsumer.cs` 96 is the fake controller module those patching tests use, not a test. `App/CustomPropEditor/ElementTreeDragDropTests.cs` 381 tests the custom prop editor's own tree, not this control.
- `App/Modeling/WireDiagramSvgBuilderTests.cs` 608, `App/Modeling/WireDiagramExportOptionsTests.cs` 100, `App/Modeling/WireDiagramExportOptionsViewModelTests.cs` 180: wire diagrams. Pinned: labels for (count, interval): (123, 50) gives 1, 50, 100, 123; (100, 50) gives 1, 50, 100; (5, 1) gives all; (5, 10) gives 1, 5; (1, 50) gives 1. Extents 100 x 1000 and 100 x 100 give portrait 816 x 1056; 1000 x 100 gives landscape 1056 x 816; width in inches is `816 / 96` at scale 1. Number of star polygons always 1.
- No upstream tests exist for templates, simple patching's counts, `DataFlowManager`, `NodeManager`, `FilterManager`, the graphical view or the name generator. Everything below is new.

### Naming

- `uniquify`: name unused stays; used gives ` - 2`, then ` - 3`. Case-sensitive: `foo` and `Foo` do not clash.
- Rename to own name is a no-op. Find/Replace with a non-matching row keeps the name (the port's fix, F138). Rename to another node's name gives ` - 2`.
- `create_node(None name)` gives "Unnamed"; two give "Unnamed", "Unnamed - 2".
- `generate_names("X-<1>", [Numeric 1, step 1, endless], 3)` gives X-1, X-2, X-3.
- `generate_names("T <1> <2>", [Numeric 1..2, Letters A count 2], 3)` gives "T 1 A", "T 1 B", "T 2 A"; wanted 10 gives only 4.
- `Numeric { start 1, end 5, endless false, step 1 }.iterations()` is 5; start 5, end 1, step -1 is 5; start 1, end 5, step -1 is endless (sign disagrees).
- `Letters` from A: index 25 Z, 26 AA, 27 AA, 28 AB. Lower-case start gives lower-case output.
- `LetterList("A b-1 ")` keeps "Ab1". `WordList` from "Left, Right" has 2 entries; from "a,,b" 3 entries; from "" one empty entry (F136); empty rule list gives no names.
- `replace_patterns(["Tree 1"], [("Tree", "Arch"), ("1", "2")])` gives "Arch 2". Empty Find skipped. Case-sensitive.

### Tree editing

- `move_node`: `[A,B,C,D]` drag A, B to below D gives `[C,D,A,B]`; drag C, D to above A gives `[C,D,A,B]`; drag A to below B gives `[B,A,C,D]`.
- `drop_target`: on row: parent = row, no index. Below expanded row: parent = row, index 0. Below collapsed row: parent = its parent, index + 1. Above row: parent = its parent, index.
- `add_child_to_parent` on a leaf with a patch removes the element and its patch. The prompt predicate `would_lose_patches` is true only for a leaf with at least one destination.
- `remove_node`: a node with another parent loses only one link and stays; the last link removes the node, its element, its properties, and unlinks its children, which survive if they have other parents. A group losing its last child becomes a leaf with a new element id and no patches.
- `invalid_children`: includes self, direct children, all ancestors through every parent; a grandchild is not invalid.
- `can_drop`: a child to its own parent as a reorder between rows is allowed on a move and refused on a copy; a node into its own descendant is refused; onto itself refused.
- Tree refusal does not use text prefixes: dropping "Tree" beside "Tree 2" succeeds (F75).
- `can_reverse`: same parent and consecutive is true; a gap, or mixed parents, is false. `reverse_selected` on `[A,B,C,D]` with B, C, D gives `[A,D,C,B]`.
- Deleting rows for a group G and its child L together: both gone, no new element, no element left without a node. Deleting L's row alone from a group with other children: one link gone.
- Creating a node named "Root" gives "Root - 2".
- Rename of two selected rows `[A, B]` to `[B, C]`: the first becomes `B - 2` because B still exists at that moment, then the second becomes `C`.
- `sort_children`: "Item 10" sorts before "Item 2".
- `duplicate_subtree`: new ids, same names uniquified; properties cloned on top node and leaves but not on child groups; a leaf appearing twice inside stays one element under two new parents; no patches or tags copied.
- Copy a row, delete it, paste: nothing is linked and the result reports nothing pasted (F35).

### Data flow and patch points (F61)

- Element alone: 1 point, unpatched.
- Element to one output: 1 point, patched.
- Element to two outputs on the same element output: 1 point, patched.
- Element to Color Breakdown with 3 outputs, 2 of which go to outputs: 3 points, 2 patched, 1 unpatched.
- Element output feeding both a 3-output filter (nothing connected) and a controller output: 3 points, all unpatched; the direct output adds none.
- Filter chain element to A to B to output: 1 point, patched.
- `set_source` forming a loop returns false and changes nothing. `remove_component` detaches both ways.
- Counts with `F62` rules: selected group G over leaves L1, L2, with a leaf also selected directly, gives items 2, groups 1, elements 2, no double counting of points.

### Simple patching (F68, F69)

- Summary text exactly: `This will patch 3 element patch points to 5 controller outputs.`, warning `WARNING: too many outputs, some will not be patched`. With 5 sources and 3 destinations: `WARNING: too many elements, some will not be patched`. Either count 0: no warning, button disabled.
- Leaves A, B, C each with 1 output; outputs 0..4 unpatched: A to 0, B to 1, C to 2, returned count 3. Reverse elements: C to 0, B to 1, A to 2. Reverse outputs: A to 4, B to 3, C to 2.
- Leaves with three colour outputs each: reverse elements gives L2 R, G, B then L1 R, G, B, keeping colour order.
- Unpatched only, with output 1 patched: A to 0, B to 2, C to 3. All outputs: A to 0, B to 1 (replacing), C to 2.
- Unconnected only, second press continues with the next unconnected points.
- `order_check`: no Order property gives TreeOrder; some gives MissingSome; all and distinct gives UseOrder and sorts by it; all with a repeat gives Duplicates.
- Unpatch with 20 patched no prompt; with 21 prompt `Are you sure you want to unpatch 21 patch points?`. Remove filters true removes the whole chain; false leaves filters with no source.

### Templates

- Megatree defaults: "Megatree" with leaves "Megatree S1".."Megatree S16", no Order. Second run: "Megatree - 2", "Megatree - 2 S1".
- Megatree pixel, 3 strings, 2 pixels, Bottom Right, no zig zag: S1P2=1, S1P1=2, S2P2=3, S2P1=4, S3P2=5, S3P1=6. With zig zag every 2: S1P2=1, S1P1=2, S2P2=4, S2P1=3, S3P2=5, S3P1=6.
- Bottom Left no zig zag: no Order property. Bottom Left zig zag every 2: tree order 1, 2, 4, 3, 5, 6.
- Pixel Grid defaults: "Grid 1", 20 "Grid 1 R{i}", each with 32 "Grid 1 R{i} C{j}": 661 nodes, 640 leaves, head Orientation Horizontal. Vertical: 32 "Grid 1 C{i}" (prefixes swapped), each with 20 "… R{j}", no Orientation.
- Pixel Grid each corner and orientation against the F108 table.
- `AddPatchingOrder` with every 3 and nine nodes: 1, 2, 3, 6, 5, 4, 7, 8, 9.
- Numbered groups: Star gives "Star" and "Star Px-1".."Star Px-40"; a second gives "Star - 2" and "Star Px-1 - 2". Arch 25, Candy Cane head "CandyCane" 25, generic "Group" and "Item-1".."Item-10".
- Icicles defaults: 50 strings, 352 leaves; the pattern "7,,9" is 7, 9. Empty pattern error text exact.
- LipSync: second run gives head "LipSync - 2" and first child "LipSync Outline - 2". Exactly 8 children in order.
- Single Item: empty gives "New Item", second "New Item - 2".
- `template_needs_followups`: Intelligent Fixture (false, false), all others (true, true).

### Intelligent fixture generation

- RGB, 3 channels with one function: Color property FullColor, one Color Breakdown with 3 outputs, 3 patch points in R, G, B order.
- 6 channels of RGB: `_16Bit` on, three Coarse/Fine filters each on one breakdown output, 6 patch points.
- Per-fixture curve: element to Dimming Curve to Breakdown. Per-colour curve: element to Breakdown to three Dimming Curves.
- Pan on two channels of one function: one Tagged filter plus one Coarse/Fine: 2 patch points. Two "None" channels: two Tagged filters with `Tag = "Ignore"`.
- Shutter with auto: Shutter filter with first open and close index values; without: Tagged. Prism always a Prism filter even with auto off (the upstream quirk).
- Dimmer with auto, lamp fixture (no mixing): Dimming filter `ConvertColorIntoDimmingIntents = true`, plus a curve when one is selected; with mixing, false and no curve.
- Colour wheel with auto, no mixing: `ConvertRGBIntoIndexCommands = true` and a discrete colour set named after the fixture.
- Count 3, prefix "MH", group "Fixtures": root "Fixtures" with "MH1", "MH2", "MH3", none at root.
- Patch points for each function come out in channel order.

### Controllers

- `move_controllers`: `[A,B,C,D]` B, C up gives `[B,C,A,D]`; down gives `[A,D,B,C]`; A, C up gives `[A,C,B,D]`; A, B up unchanged; C, D down unchanged. Order survives a following delete (the port's fix).
- `set_output_count` shrinking drops patches on removed outputs and keeps others; growing names `Output {n}`.
- `insert_outputs(at 1, count 2)` on outputs 0..2: new at 1 and 2; old 1 is now 3 with its patch and identity; names renumbered unless the module names them.
- `remove_outputs` re-indexes survivors and keeps their patches, and lookup from component to (controller, index) stays right.
- After Remove Channels on outputs {3} of 10, the selection is output index 3 (now the old output 4); on {9} of 10, the selection is empty (F95).
- The logical selection keeps outputs hidden by a collapse: stats include them, and the highlighted rows do not (F81).
- `find_elements`, `find_outputs` through a filter chain, including a chain with two destinations.
- Menu state tables F86, F92, F93 with singular and plural wording.
- `commit` removes orphan filters, plus a chain under one; unconnected new filter removed; connected kept.

### Graphical view

- `graph_scene`: a child selected with its parent appears once, nested; a child also having an unselected parent appears as its own root too.
- Layout numbers: W = 1000, one leaf root: width 200, height 20. Group with 2 leaves: height 20 + 2×20 + 6 + 6 = 72 by the rule in F115, children width 190.
- Ports: 1 output at 0.5; 3 outputs at 0.1, 0.5, 0.9 of height.
- Filter depth: element to A to B gives 1 and 2, and no element gives 0. Depth-0 filter grid column count `min(n, FW / 38)`.
- Stub rules: output whose source is not displayed gets a left stub; leaf with destination not displayed gets a right stub; a real line replaces a stub when both are displayed.
- Connect refusals: self, cycle, onto a group, onto a controller. Replacing an input's old source.
- Delete filter removes its links both ways.
- Deleting a left stub resets that output's source; a right stub changes nothing. Deleting a filter and its attached line together works.
- A group selected together with its child lists each filter once (F112). A filter connected after its shape existed gets its real depth at the next layout (F115).
- A collapsed group's hidden leaf keeps its lines, ended at the group's right edge midpoint (F124).
- Zoom to fit formula: view 500 x 500, content 1000 x 1000 gives 50% (upstream 49, its scrollbar workaround); content smaller than view gives 100%; extreme content clamps at 20%.

### Performance guards

`#[test]`s with time budgets, ignored by default in debug: generating a 1000 x 100 grid under 1 second in release; `element_stats` over 100,000 leaves under 50 ms from the cached tables; `graph_scene` for 10,000 shapes under 100 ms; `move_controllers` and `set_source` constant in the size of the tree.

## Windows-only omissions

- **NShape and its machinery.** The library itself, plus `RoleBasedSecurityManager` and security domains (replaced by plain rules: filters move; leaves, filters and outputs connect; filters and lines delete), layers (replaced by a hidden flag), cached repository, `Project`, `DiagramSetController`, `NShapeLibraryInitializer`, cursors `MoveShapeCursor`, `MovePointCursor`, `HandCursor`. It is GPL, and the Mac view is a native rebuild.
- **Unreachable or accidental NShape shortcuts.** Ctrl+Insert and Shift+Insert (NShape's own shape copy and paste), Ctrl+Shift+C/V/X, and the F2 caption editor (no shape domain grants the Data permission). See F125, F127.
- **WinForms theming and layout.** `ThemeUpdateControls`, `ThemeGroupBoxRenderer`, `ThemeComboBoxRenderer`, `ThemeToolStripRenderer`, owner-drawn combo items and list items, `ScalingTools`, `TableLayoutPanel`, `FlowLayoutPanel`, `AutoScale`. The system does these.
- **Access keys.** `&Add`, `&Delete`, `Re&name` and the like. macOS menus have no mnemonics.
- **TreeView workarounds.** The "VIRT" placeholder, eviction on collapse, the `WM_ERASEBKGND` suppression and `WM_PAINT` overlay, `SendMessage` `WM_VSCROLL` for drag auto-scroll, native-selection cancelling, the 10-pt "full-width hit test" click rule. `NSOutlineView` covers each.
- **`contextMenuStripDragging` ("Move Here", "Copy Here").** Defined in the designer with no handlers and never shown (ElementTree.Designer.cs:243-258). Dead on Windows too.
- **`HandleException` message boxes in the tree control.** A catch-all around mouse and key handling (MultiSelectTreeview.cs:1186-1195). Not needed. Errors from Rust come back as `FfiError` messages.
- **Interop plumbing.** `ElementHost.EnableModelessKeyboardInterop`, `WindowInteropHelper`, Catel view models and `ITypeFactory`, Orc wizard services, `ThemeManager.SynchronizeTheme`, `IBaseColorSchemeService`, `IAccentColorService`: WPF and Catel hosting only.
- **`Process.Start` with `UseShellExecute`.** Replaced by `NSWorkspace.shared.open` (help pages, SVG results).
- **`SystemSounds.Hand`** on refused paste. Becomes `NSSound.beep()`, so kept in spirit.
- **`ShowInTaskbar`, `StartPosition`, `MinimizeBox`, `MaximizeBox`, size grips.** Window chrome that macOS handles by its own rules.
- **Decorative moving heads in the wizard (F140).** Purely visual, using WPF 3D; not Windows-specific in principle, but they have no function. Omitted at first; easy to add later with crates/vixen-render. The owner can veto.
- **Reflection discovery of templates and helpers.** Replaced by fixed lists, since there are no plugins here (F99, sibling spec F4).
- **Help links on `http://www.vixenlights.com/...`.** Kept, though old paths may no longer exist on the site. Not an omission; recorded so the parity check does not mistake a dead link for a bug.

