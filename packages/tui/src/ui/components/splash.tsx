import { memo } from "react";
import { Match, Option } from "effect";
import type { ResolvedTheme } from "#theme/theme";

export interface SplashProps {
	readonly theme: ResolvedTheme;
	readonly error: Option.Option<string>;
}

export const Splash = memo(function Splash(props: SplashProps) {
	const isNotGitRepositoryError = Option.match(props.error, {
		onNone: () => false,
		onSome: (error) => /not a git repository/i.test(error),
	});

	const subtitle = Option.match(props.error, {
		onNone: () => "No changed files in working tree",
		onSome: (error) =>
			Match.value(error).pipe(
				Match.when(
					(message) => /not a git repository/i.test(message),
					() => "Not a git repo, init to use vigil.",
				),
				Match.orElse((message) => message),
			),
	});

	return (
		<box flexGrow={1} justifyContent="center" alignItems="center">
			<box flexDirection="column" rowGap={1} alignItems="center">
				<ascii-font text="vigil" font="block" color={props.theme.text} />
				<text fg={props.theme.textMuted}>{subtitle}</text>
				{isNotGitRepositoryError ? (
					<text fg={props.theme.textMuted}>Press i to git init.</text>
				) : null}
			</box>
		</box>
	);
});
