import type { ScrollBoxRenderable, SyntaxStyle } from "@opentui/core";
import { Option, pipe } from "effect";
import { memo, type RefObject, useMemo } from "react";
import { isFileStaged } from "#data/git";
import { splitDiffIntoHunkBlocks } from "#diff/hunks";
import type { ResolvedTheme } from "#theme/theme";
import type { FileEntry } from "#tui/types";
import { getStatusColor, type SidebarItem } from "#ui/sidebar";

type SidebarHeaderItem = Extract<SidebarItem, { kind: "header" }>;
type SidebarFileItem = Extract<SidebarItem, { kind: "file" }>;

interface SidebarHeaderRowProps {
	readonly item: SidebarHeaderItem;
	readonly theme: ResolvedTheme;
	readonly onToggleDirectory: (path: string) => void;
}

const SidebarHeaderRow = memo(function SidebarHeaderRow(
	props: SidebarHeaderRowProps,
) {
	const headerPrefix = "  ".repeat(props.item.depth);
	return (
		<box
			paddingX={1}
			onMouseDown={(event) => {
				event.preventDefault();
				props.onToggleDirectory(props.item.path);
			}}
		>
			<text fg={props.theme.textMuted}>
				<span fg={props.theme.borderActive}>
					{headerPrefix}
					{props.item.collapsed ? "▸ " : "▾ "}
				</span>
				{props.item.label}
			</text>
		</box>
	);
});

interface SidebarFileRowProps {
	readonly item: SidebarFileItem;
	readonly theme: ResolvedTheme;
	readonly selectedFilePath: Option.Option<string>;
	readonly onSelectFilePath: (path: string) => void;
}

const SidebarFileRow = memo(function SidebarFileRow(props: SidebarFileRowProps) {
	const { file } = props.item;
	const selected =
		Option.isSome(props.selectedFilePath) &&
		props.selectedFilePath.value === file.path;
	const staged = isFileStaged(file.status);
	const rowBackground = selected
		? staged
			? props.theme.diffAddedLineNumberBg
			: props.theme.backgroundElement
		: staged
			? props.theme.diffAddedBg
			: "transparent";
	const filePrefix = "  ".repeat(props.item.depth);

	return (
		<box
			paddingX={1}
			backgroundColor={rowBackground}
			onMouseDown={(event) => {
				event.preventDefault();
				props.onSelectFilePath(file.path);
			}}
		>
			<text>
				<span fg={props.theme.borderSubtle}>{filePrefix}</span>
				<span fg={getStatusColor(file.status, props.theme)}>{file.status}</span>{" "}
				<span fg={selected || staged ? props.theme.text : props.theme.textMuted}>
					{props.item.label}
				</span>
			</text>
		</box>
	);
});

interface SidebarPanelProps {
	readonly theme: ResolvedTheme;
	readonly files: FileEntry[];
	readonly sidebarItems: SidebarItem[];
	readonly selectedFilePath: Option.Option<string>;
	readonly onToggleDirectory: (path: string) => void;
	readonly onSelectFilePath: (path: string) => void;
}

const SidebarPanel = memo(function SidebarPanel(props: SidebarPanelProps) {
	return (
		<box
			width={38}
			border
			borderStyle="rounded"
			borderColor={props.theme.border}
			marginRight={1}
			flexDirection="column"
			backgroundColor={props.theme.backgroundPanel}
		>
			<box paddingX={1} marginBottom={1}>
				<text fg={props.theme.text}>
					<strong>Changed Files ({props.files.length})</strong>
				</text>
			</box>

			<scrollbox flexGrow={1}>
				{props.sidebarItems.map((item) =>
					item.kind === "header" ? (
						<SidebarHeaderRow
							key={item.key}
							item={item}
							theme={props.theme}
							onToggleDirectory={props.onToggleDirectory}
						/>
					) : (
						<SidebarFileRow
							key={item.key}
							item={item}
							theme={props.theme}
							selectedFilePath={props.selectedFilePath}
							onSelectFilePath={props.onSelectFilePath}
						/>
					),
				)}
			</scrollbox>
		</box>
	);
});

interface SidebarRailProps {
	readonly theme: ResolvedTheme;
	readonly onToggleSidebar: () => void;
}

const SidebarRail = memo(function SidebarRail(props: SidebarRailProps) {
	return (
		<box
			width={3}
			border
			borderStyle="rounded"
			borderColor={props.theme.border}
			marginRight={1}
			justifyContent="center"
			alignItems="center"
			backgroundColor={props.theme.backgroundPanel}
			onMouseDown={(event) => {
				event.preventDefault();
				props.onToggleSidebar();
			}}
		>
			<text fg={props.theme.textMuted}>▸</text>
		</box>
	);
});

interface DiffPanelProps {
	readonly theme: ResolvedTheme;
	readonly syntaxStyle: SyntaxStyle;
	readonly diffViewMode: "split" | "unified";
	readonly selectedFile: FileEntry | null;
	readonly loading: boolean;
	readonly error: Option.Option<string>;
	readonly isCommitModalOpen: boolean;
	readonly diffScrollRef: RefObject<ScrollBoxRenderable | null>;
}

const DiffPanel = memo(function DiffPanel(props: DiffPanelProps) {
	const selectedFileHunkDiffs = useMemo(() => {
		if (!props.selectedFile?.diff.trim()) {
			return [];
		}
		return splitDiffIntoHunkBlocks(props.selectedFile.diff);
	}, [props.selectedFile]);

	return (
		<box
			flexGrow={1}
			border
			borderStyle="rounded"
			borderColor={props.theme.border}
			flexDirection="column"
			backgroundColor={props.theme.backgroundPanel}
		>
			<box paddingX={1} marginBottom={1}>
				<text fg={props.theme.text}>
					<strong>
						{props.selectedFile ? props.selectedFile.label : "No file selected"}
					</strong>
				</text>
			</box>

			<box flexGrow={1} padding={1}>
				{props.loading ? (
					<text fg={props.theme.textMuted}>Loading git status...</text>
				) : Option.isSome(props.error) ? (
					<text fg={props.theme.error}>{props.error.value}</text>
				) : !props.selectedFile ? (
					<text fg={props.theme.textMuted}>No changed files found.</text>
				) : props.selectedFile.diff.trim() ? (
					<scrollbox
						ref={props.diffScrollRef}
						flexGrow={1}
						focused={!props.isCommitModalOpen}
						verticalScrollbarOptions={{
							trackOptions: {
								backgroundColor: props.theme.backgroundElement,
								foregroundColor: props.theme.borderActive,
							},
						}}
					>
						<box flexDirection="column">
							{selectedFileHunkDiffs.map((hunkDiff, hunkIndex) => (
								<box
									key={`${props.selectedFile?.path}:${hunkIndex}`}
									flexDirection="column"
								>
									<diff
										diff={hunkDiff}
										{...(props.selectedFile?.filetype
											? { filetype: props.selectedFile.filetype }
											: {})}
										syntaxStyle={props.syntaxStyle}
										view={props.diffViewMode}
										showLineNumbers
										width="100%"
										wrapMode="word"
										fg={props.theme.text}
										addedBg={props.theme.diffAddedBg}
										removedBg={props.theme.diffRemovedBg}
										contextBg={props.theme.diffContextBg}
										addedSignColor={props.theme.diffHighlightAdded}
										removedSignColor={props.theme.diffHighlightRemoved}
										lineNumberFg={props.theme.diffLineNumber}
										lineNumberBg={props.theme.diffContextBg}
										addedLineNumberBg={props.theme.diffAddedLineNumberBg}
										removedLineNumberBg={props.theme.diffRemovedLineNumberBg}
									/>
									{hunkIndex < selectedFileHunkDiffs.length - 1 ? (
										<box height={1} backgroundColor={props.theme.background} />
									) : null}
								</box>
							))}
						</box>
					</scrollbox>
				) : (
					<text fg={props.theme.textMuted}>
						{props.selectedFile.note ?? "No diff preview available."}
					</text>
				)}
			</box>
		</box>
	);
});

export interface ReviewerProps {
	readonly theme: ResolvedTheme;
	readonly syntaxStyle: SyntaxStyle;
	readonly files: FileEntry[];
	readonly sidebarItems: SidebarItem[];
	readonly selectedFile: FileEntry | null;
	readonly loading: boolean;
	readonly diffViewMode: "split" | "unified";
	readonly error: Option.Option<string>;
	readonly isCommitModalOpen: boolean;
	readonly diffScrollRef: RefObject<ScrollBoxRenderable | null>;
	readonly onToggleDirectory: (path: string) => void;
	readonly onSelectFilePath: (path: string) => void;
	readonly sidebarOpen: boolean;
	readonly onToggleSidebar: () => void;
}

export const Reviewer = memo(function Reviewer(props: ReviewerProps) {
	const selectedFilePath = pipe(
		Option.fromNullable(props.selectedFile),
		Option.map((file) => file.path),
	);

	return (
		<box flexDirection="row" flexGrow={1}>
			{props.sidebarOpen ? (
				<SidebarPanel
					theme={props.theme}
					files={props.files}
					sidebarItems={props.sidebarItems}
					selectedFilePath={selectedFilePath}
					onToggleDirectory={props.onToggleDirectory}
					onSelectFilePath={props.onSelectFilePath}
				/>
			) : (
				<SidebarRail
					theme={props.theme}
					onToggleSidebar={props.onToggleSidebar}
				/>
			)}
			<DiffPanel
				theme={props.theme}
				syntaxStyle={props.syntaxStyle}
				diffViewMode={props.diffViewMode}
				selectedFile={props.selectedFile}
				loading={props.loading}
				error={props.error}
				isCommitModalOpen={props.isCommitModalOpen}
				diffScrollRef={props.diffScrollRef}
			/>
		</box>
	);
});
