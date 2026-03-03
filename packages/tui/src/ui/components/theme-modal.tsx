import type { RGBA, ScrollBoxRenderable } from "@opentui/core";
import { memo, useRef } from "react";
import type { ResolvedTheme } from "#theme/theme.ts";
import { useScrollFollowSelection } from "#ui/hooks/use-scroll-follow-selection.ts";

export interface ThemeModalProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly themes: ReadonlyArray<string>;
	readonly selectedThemeName: string;
	readonly searchQuery: string;
	readonly onSearchQueryChange: (value: string) => void;
	readonly onSelectTheme: (themeName: string) => void;
}

export const ThemeModal = memo(function ThemeModal(props: ThemeModalProps) {
	const themeScrollRef = useRef<ScrollBoxRenderable | null>(null);
	const selectedRowId = props.themes.includes(props.selectedThemeName)
		? `theme-row:${props.selectedThemeName}`
		: null;

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
				height={22}
				padding={1}
				gap={1}
				border
				borderStyle="rounded"
				borderColor={props.theme.borderActive}
				backgroundColor={props.theme.backgroundPanel}
				flexDirection="column"
			>
				<text fg={props.theme.text}>
					<strong>Themes</strong>
				</text>
				<input
					value={props.searchQuery}
					onInput={props.onSearchQueryChange}
					placeholder="Filter themes..."
					focused
					width="100%"
					backgroundColor={props.theme.backgroundElement}
					focusedBackgroundColor={props.theme.backgroundElement}
					textColor={props.theme.text}
					focusedTextColor={props.theme.text}
					placeholderColor={props.theme.textMuted}
				/>
				<scrollbox ref={themeScrollRef} flexGrow={1}>
					{props.themes.length === 0 ? (
						<box paddingX={1}>
							<text fg={props.theme.textMuted}>No matching themes.</text>
						</box>
					) : (
						props.themes.map((themeName) => {
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
										fg={
											active
												? props.theme.selectedListItemText
												: props.theme.text
										}
									>
										{themeName}
									</text>
								</box>
							);
						})
					)}
				</scrollbox>
			</box>
		</box>
	);
});
