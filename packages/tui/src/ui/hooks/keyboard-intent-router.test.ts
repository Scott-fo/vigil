import { describe, expect, mock, test } from "bun:test";
import { FileEntry } from "#tui/types.ts";
import {
	routeKeyboardIntent,
	type KeyboardIntentRouterActions,
} from "#ui/hooks/keyboard-intent-router.ts";

function createActions(): KeyboardIntentRouterActions {
	return {
		destroyRenderer: mock(),
		toggleSidebar: mock(),
		toggleDiffViewMode: mock(),
		closeBlameView: mock(),
		openBlameCommitCompare: mock(),
		scrollBlameView: mock(),
		closeCommitModal: mock(),
		openCommitModal: mock(),
		closeDiscardModal: mock(),
		openDiscardModal: mock((_selectedFile: FileEntry) => {}),
		confirmDiscardModal: mock(),
		closeHelpModal: mock(),
		openHelpModal: mock(),
		initializeGitRepository: mock(),
		openThemeModal: mock(),
		openBranchCompareModal: mock(),
		openCommitSearchModal: mock(),
		closeThemeModal: mock(),
		closeBranchCompareModal: mock(),
		closeCommitSearchModal: mock(),
		confirmThemeModal: mock(),
		confirmBranchCompareModal: mock(),
		confirmCommitSearchModal: mock(),
		moveThemeSelection: mock((_direction: 1 | -1) => {}),
		moveBranchSelection: mock((_direction: 1 | -1) => {}),
		moveCommitSearchSelection: mock((_direction: 1 | -1) => {}),
		switchBranchField: mock(),
		syncRemote: mock((_direction: "pull" | "push") => {}),
		resetReviewMode: mock(),
		scrollDiffHalfPage: mock((_direction: "up" | "down") => {}),
		moveDiffSelection: mock((_direction: 1 | -1) => {}),
		focusSidebarPane: mock(),
		focusDiffPane: mock(),
		openSelectedFile: mock((_filePath: string) => {}),
		openSelectedDiffLine: mock((_filePath: string, _lineNumber: number) => {}),
		toggleSelectedFileStage: mock((_selectedFile: FileEntry) => {}),
		selectFilePath: mock((_path: string) => {}),
	};
}

describe("routeKeyboardIntent", () => {
	test("invokes destroyRenderer for DestroyRenderer intents", () => {
		const actions = createActions();

		routeKeyboardIntent({ _tag: "DestroyRenderer" }, actions);

		expect(actions.destroyRenderer).toHaveBeenCalledTimes(1);
	});

	test("passes payloads through for parameterized intents", () => {
		const actions = createActions();
		const file = FileEntry.make({
			status: "M ",
			path: "src/app.tsx",
			label: "src/app.tsx",
		});

		routeKeyboardIntent(
			{
				_tag: "ToggleSelectedFileStage",
				file,
			},
			actions,
		);

		expect(actions.toggleSelectedFileStage).toHaveBeenCalledWith(file);
	});
});
