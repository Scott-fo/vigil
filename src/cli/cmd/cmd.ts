export interface CliCommand<TArgs> {
	command: string;
	describe: string;
	usage: () => string;
	parse: (argv: string[]) => TArgs;
	handler: (args: TArgs) => Promise<void> | void;
}

export function cmd<TArgs>(input: CliCommand<TArgs>): CliCommand<TArgs> {
	return input;
}
