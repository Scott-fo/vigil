import type { AppProps } from "#tui/types.ts";
import { App } from "#ui/app.tsx";
import { useBootResources } from "#ui/hooks/use-boot-resources.ts";

type BootAppProps = Omit<AppProps, "themeCatalog" | "themeMode" | "themeName">;

export function BootApp(props: BootAppProps) {
	const bootResources = useBootResources();

	return (
		<App
			themeCatalog={bootResources.themeCatalog}
			themeMode={bootResources.themeMode}
			themeName={bootResources.themeName}
			chooserFilePath={props.chooserFilePath}
			initialBlameTarget={props.initialBlameTarget}
			daemonConnection={props.daemonConnection}
		/>
	);
}
