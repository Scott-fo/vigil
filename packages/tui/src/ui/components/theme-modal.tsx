import { RGBA, type ScrollBoxRenderable } from "@opentui/core";
import { memo, useRef } from "react";
import type { ResolvedTheme } from "#theme/theme";
import { useScrollFollowSelection } from "#ui/hooks/use-scroll-follow-selection";

export interface ThemeModalProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly themes: ReadonlyArray<string>;
	readonly selectedThemeName: string;
	readonly onSelectTheme: (themeName: string) => void;
}

export const ThemeModal = memo(function ThemeModal(props: ThemeModalProps) {
	const themeScrollRef = useRef<ScrollBoxRenderable | null>(null);
	const selectedRowId = `theme-row:${props.selectedThemeName}`;

	useScrollFollowSelection({
		scrollRef: themeScrollRef,
		selectedRowId,
	});

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
			zIndex={115}
		>
			<box
				width={56}
				height={20}
				border
				borderStyle="rounded"
				borderColor={props.theme.borderActive}
				backgroundColor={props.theme.backgroundPanel}
				padding={1}
				flexDirection="column"
			>
				<text fg={props.theme.text}>
					<strong>Themes</strong>
				</text>
				<box marginTop={1} marginBottom={1}>
					<text fg={props.theme.textMuted}>
						Use ↑↓ to preview, Enter to confirm, Esc to cancel.
					</text>
				</box>
				<scrollbox ref={themeScrollRef} flexGrow={1}>
					{props.themes.map((themeName) => {
						const active = themeName === props.selectedThemeName;
						return (
							<box
								key={themeName}
								id={`theme-row:${themeName}`}
								paddingX={1}
								backgroundColor={active ? props.theme.primary : "transparent"}
								onMouseDown={(event) => {
									event.preventDefault();
									props.onSelectTheme(themeName);
								}}
							>
								<text
									fg={active ? props.theme.selectedListItemText : props.theme.text}
								>
									{themeName}
								</text>
							</box>
						);
					})}
				</scrollbox>
			</box>
		</box>
	);
});
