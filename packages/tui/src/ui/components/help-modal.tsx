import { RGBA } from "@opentui/core";
import { memo } from "react";
import type { ResolvedTheme } from "#theme/theme";

interface KeybindRow {
	readonly keys: string;
	readonly description: string;
}

const KEYBIND_ROWS: ReadonlyArray<KeybindRow> = [
	{ keys: "j / k, ↑ / ↓", description: "Navigate changed files" },
	{ keys: "space", description: "Stage / unstage selected file" },
	{ keys: "enter / e / o", description: "Open selected file in editor" },
	{ keys: "tab", description: "Toggle split / unified diff" },
	{ keys: "Ctrl+u / Ctrl+d", description: "Scroll diff up / down" },
	{ keys: "Ctrl+b", description: "Toggle sidebar" },
	{ keys: "c", description: "Open commit dialog (if files staged)" },
	{ keys: "p / P", description: "Pull / push" },
	{ keys: "t", description: "Open theme picker" },
	{ keys: "q or esc", description: "Quit reviewer" },
];

export interface HelpModalProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
}

export const HelpModal = memo(function HelpModal(props: HelpModalProps) {
	return (
		<box
			position="absolute"
			left={0}
			top={0}
			width="100%"
			height="100%"
			justifyContent="center"
			alignItems="center"
			backgroundColor={props.modalBackdropColor}
			zIndex={110}
		>
			<box
				width={78}
				border
				borderStyle="rounded"
				borderColor={props.theme.borderActive}
				backgroundColor={props.theme.backgroundPanel}
				padding={1}
				flexDirection="column"
			>
				<text fg={props.theme.text}>
					<strong>Keybinds</strong>
				</text>
				<box marginTop={1} marginBottom={1}>
					<text fg={props.theme.textMuted}>
						Press <strong>Esc</strong> to close this help dialog.
					</text>
				</box>
				<box flexDirection="column">
					{KEYBIND_ROWS.map((row) => (
						<box key={row.keys}>
							<text fg={props.theme.text}>
								<span fg={props.theme.info}>{row.keys.padEnd(17, " ")}</span>
								{row.description}
							</text>
						</box>
					))}
				</box>
			</box>
		</box>
	);
});
