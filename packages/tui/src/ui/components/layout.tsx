import type { ReviewerProps } from "#ui/components/reviewer.tsx";
import { Reviewer } from "#ui/components/reviewer.tsx";
import { Splash } from "#ui/components/splash.tsx";
import type { UiStatus } from "#ui/state.ts";
import type { ResolvedTheme } from "#theme/theme.ts";

interface LayoutProps {
	readonly reviewerProps: ReviewerProps;
	readonly theme: ResolvedTheme;
	readonly uiStatus: UiStatus;
}

export function Layout(props: LayoutProps) {
	return props.uiStatus.showSplash ? (
		<Splash theme={props.theme} error={props.uiStatus.error} />
	) : (
		<Reviewer {...props.reviewerProps} />
	);
}
