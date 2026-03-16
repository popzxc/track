#!/usr/bin/env bun

import { formatCliError } from './output/format'
import { runCreateCommand } from './commands/create'

async function main() {
  const argv = process.argv.slice(2)
  const output = await runCreateCommand(argv)
  console.log(output)
}

main().catch((error) => {
  console.error(formatCliError(error))
  process.exitCode = 1
})
