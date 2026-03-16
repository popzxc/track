import { access, readdir } from 'node:fs/promises'
import { constants as fsConstants } from 'node:fs'
import { basename, join } from 'node:path'

import type { ProjectInfo, TrackConfig } from '@track/shared'

const IGNORED_DIRECTORIES = new Set([
  '.git',
  'node_modules',
  'dist',
  'target',
  '.next',
  '.turbo',
  '.venv',
])

async function pathExists(pathValue: string): Promise<boolean> {
  try {
    await access(pathValue, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

export class ProjectService {
  // TODO: Add an optional manual refresh cache if repeated scans become noticeably slow.
  async discoverProjects(config: TrackConfig): Promise<ProjectInfo[]> {
    const discoveredProjects = new Map<string, ProjectInfo>()

    for (const projectRoot of config.projectRoots) {
      const repos = await this.scanForGitRepos(projectRoot)

      for (const repoPath of repos) {
        const canonicalName = basename(repoPath)
        const projectKey = canonicalName.toLowerCase()

        if (!discoveredProjects.has(projectKey)) {
          discoveredProjects.set(projectKey, {
            canonicalName,
            path: repoPath,
            aliases: [],
          })
        }
      }
    }

    for (const [alias, canonicalName] of Object.entries(config.projectAliases)) {
      const project = discoveredProjects.get(canonicalName.toLowerCase())
      if (project) {
        project.aliases.push(alias)
      }
    }

    return [...discoveredProjects.values()].sort((left, right) =>
      left.canonicalName.localeCompare(right.canonicalName),
    )
  }

  private async scanForGitRepos(rootPath: string): Promise<string[]> {
    if (!(await pathExists(rootPath))) {
      return []
    }

    // We intentionally stick to filesystem-based `.git` detection here so discovery
    // stays dependency-free and predictable for a localhost tool.
    const discoveredRepos: string[] = []
    const pendingDirectories = [rootPath]

    while (pendingDirectories.length > 0) {
      const currentDirectory = pendingDirectories.pop()
      if (!currentDirectory) {
        continue
      }

      const entries = await readdir(currentDirectory, { withFileTypes: true }).catch(() => [])

      if (entries.some((entry) => entry.name === '.git')) {
        discoveredRepos.push(currentDirectory)
      }

      for (const entry of entries) {
        if (!entry.isDirectory()) {
          continue
        }

        if (IGNORED_DIRECTORIES.has(entry.name)) {
          continue
        }

        pendingDirectories.push(join(currentDirectory, entry.name))
      }
    }

    return discoveredRepos
  }
}
