import { memo } from "react";
import type { ResolvedTheme } from "#theme/theme";

export interface SplashProps {
	readonly theme: ResolvedTheme;
}

export const Splash = memo(function Splash(props: SplashProps) {
	return (
		<box flexGrow={1} justifyContent="center" alignItems="center">
			<box flexDirection="column" rowGap={1} alignItems="center">
				<ascii-font text="reviewer" font="block" color={props.theme.text} />
				<text fg={props.theme.textMuted}>
					Initialise git repo to use Reviewer
				</text>
			</box>
		</box>
	);
});
