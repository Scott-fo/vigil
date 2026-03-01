import type { CliRenderer, ScrollBoxRenderable } from "@opentui/core";
import { useKeyboard } from "@opentui/react";
import type { RefObject } from "react";
import type { FileEntry } from "#tui/types";

interface UseAppKeyboardInputOptions {
	renderer: CliRenderer;
	isCommitModalOpen: boolean;
	stagedFileCount: number;
	visibleFilePaths: string[];
	selectedVisibleIndex: number;
	selectedFile: FileEntry | null;
	diffScrollRef: RefObject<ScrollBoxRenderable | null>;
	closeCommitModal: () => void;
	openCommitModal: () => void;
	cycleTheme: (direction: 1 | -1) => void;
	syncRemote: (direction: "pull" | "push") => void;
	setSelectedPath: (path: string | null) => void;
	openSelectedFile: (filePath: string) => void;
	toggleSelectedFileStage: (file: FileEntry) => void;
}

export function useAppKeyboardInput(options: UseAppKeyboardInputOptions) {
	useKeyboard((key) => {
		if (key.ctrl && key.name === "c") {
			options.renderer.destroy();
			return;
		}

		if (options.isCommitModalOpen) {
			if (key.name === "escape") {
				options.closeCommitModal();
			}
			return;
		}

		if (key.name === "escape" || key.name === "q") {
			options.renderer.destroy();
			return;
		}

		if (!key.ctrl && !key.meta && key.name === "c") {
			if (options.stagedFileCount > 0) {
				options.openCommitModal();
			}
			return;
		}

		if (!key.ctrl && !key.meta && key.name === "t") {
			options.cycleTheme(key.shift ? -1 : 1);
			return;
		}

		if (!key.ctrl && !key.meta && key.name === "p") {
			options.syncRemote(key.shift ? "push" : "pull");
			return;
		}

		if (key.ctrl && (key.name === "u" || key.name === "d")) {
			const step = Math.max(6, Math.floor(options.renderer.height * 0.45));
			options.diffScrollRef.current?.scrollBy({
				x: 0,
				y: key.name === "u" ? -step : step,
			});
			return;
		}

		if (
			options.visibleFilePaths.length === 0 ||
			options.selectedVisibleIndex === -1
		) {
			return;
		}

		if (key.name === "enter" || key.name === "return") {
			if (options.selectedFile) {
				options.openSelectedFile(options.selectedFile.path);
			}
			return;
		}

		if (!key.ctrl && !key.meta && (key.name === "space" || key.name === " ")) {
			if (options.selectedFile) {
				options.toggleSelectedFileStage(options.selectedFile);
			}
			return;
		}

		if (key.name === "down" || key.name === "j") {
			const nextIndex = Math.min(
				options.selectedVisibleIndex + 1,
				options.visibleFilePaths.length - 1,
			);
			options.setSelectedPath(options.visibleFilePaths[nextIndex] ?? null);
			return;
		}

		if (key.name === "up" || key.name === "k") {
			const nextIndex = Math.max(options.selectedVisibleIndex - 1, 0);
			options.setSelectedPath(options.visibleFilePaths[nextIndex] ?? null);
		}
	});
}
