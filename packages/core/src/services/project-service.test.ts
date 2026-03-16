import { describe, expect, it } from 'bun:test'
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { ProjectService } from './project-service'

describe('ProjectService', () => {
  it('discovers git repositories under configured roots', async () => {
    const rootDirectory = await mkdtemp(join(tmpdir(), 'track-projects-'))
    await mkdir(join(rootDirectory, 'workspace', 'project-a', '.git'), { recursive: true })
    await mkdir(join(rootDirectory, 'workspace', 'nested', 'project-b', '.git'), { recursive: true })

    const service = new ProjectService()
    const projects = await service.discoverProjects({
      projectRoots: [rootDirectory],
      projectAliases: {},
      ai: {
        provider: 'openai',
        openai: {},
      },
    })

    expect(projects.map((project) => project.canonicalName)).toEqual(['project-a', 'project-b'])
  })

  it('attaches aliases to the matching canonical project', async () => {
    const rootDirectory = await mkdtemp(join(tmpdir(), 'track-projects-'))
    await mkdir(join(rootDirectory, 'workspace', 'project-a'), { recursive: true })
    await writeFile(join(rootDirectory, 'workspace', 'project-a', '.git'), 'gitdir: /tmp/example')

    const service = new ProjectService()
    const projects = await service.discoverProjects({
      projectRoots: [rootDirectory],
      projectAliases: {
        'proj-a': 'project-a',
      },
      ai: {
        provider: 'openai',
        openai: {},
      },
    })

    expect(projects[0]?.aliases).toEqual(['proj-a'])
  })
})
