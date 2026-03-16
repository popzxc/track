import type { ParsedTaskCandidate, ProjectInfo, TaskSource } from '@track/shared'

import { TrackError } from '../errors'
import type { AiTaskParser } from '../ai/provider'
import { ConfigService } from './config-service'
import { ProjectService } from './project-service'
import { FileTaskRepository } from '../storage/file-task-repository'

// =============================================================================
// Parsed Project Validation
// =============================================================================
//
// The model is allowed to infer, but not to invent. This guard turns the model
// output back into an application decision by proving that the chosen project is
// both high-confidence and present in the discovered project set.
//
function normalizeCanonicalProject(
  candidate: ParsedTaskCandidate,
  allowedProjects: ProjectInfo[],
): { project: string; priority: ParsedTaskCandidate['priority']; description: string } {
  // The CLI must fail loudly when project selection is fuzzy, otherwise we risk
  // quietly writing tasks into the wrong repository folder.
  if (candidate.project === null || candidate.confidence === 'low') {
    throw new TrackError(
      'INVALID_PROJECT_SELECTION',
      'Could not determine a valid project from your input. Please mention one of the allowed project names or aliases more explicitly.',
    )
  }

  const matchedProject = allowedProjects.find(
    (project) => project.canonicalName.toLowerCase() === candidate.project?.toLowerCase(),
  )

  if (!matchedProject) {
    throw new TrackError(
      'INVALID_PROJECT_SELECTION',
      'Could not determine a valid project from your input. Please mention one of the allowed project names or aliases more explicitly.',
    )
  }

  return {
    project: matchedProject.canonicalName,
    priority: candidate.priority,
    description: candidate.description.trim(),
  }
}

export class TaskCaptureService {
  constructor(
    private readonly dependencies: {
      aiTaskParser: AiTaskParser
      configService: ConfigService
      projectService: ProjectService
      taskRepository: FileTaskRepository
    },
  ) {}

  async createTaskFromText(input: { rawText: string; source?: TaskSource }) {
    if (input.rawText.trim().length === 0) {
      throw new TrackError('EMPTY_INPUT', 'Please provide a task description.')
    }

    // =============================================================================
    // Capture Flow
    // =============================================================================
    //
    // We keep creation intentionally linear:
    // 1. load the shared config
    // 2. discover real projects from disk
    // 3. ask the AI to choose only from that set
    // 4. validate the AI result before any file write happens
    //
    // The capture flow favors correctness over clever shortcuts:
    // discover the current project set, ask the parser to choose from it,
    // then persist only after the choice has been validated.
    const config = await this.dependencies.configService.loadConfig()
    if (config.projectRoots.length === 0) {
      throw new TrackError('NO_PROJECT_ROOTS', 'No project roots configured.')
    }

    const discoveredProjects = await this.dependencies.projectService.discoverProjects(config)
    if (discoveredProjects.length === 0) {
      throw new TrackError('NO_PROJECTS_DISCOVERED', 'No git repositories found under configured roots.')
    }

    const parsedCandidate = await this.dependencies.aiTaskParser.parseTask({
      rawText: input.rawText,
      allowedProjects: discoveredProjects.map((project) => ({
        canonicalName: project.canonicalName,
        aliases: project.aliases,
      })),
    })

    const normalizedCandidate = normalizeCanonicalProject(parsedCandidate, discoveredProjects)
    return this.dependencies.taskRepository.createTask({
      project: normalizedCandidate.project,
      priority: normalizedCandidate.priority,
      description: normalizedCandidate.description,
      source: input.source,
    })
  }
}

export function validateParsedTaskCandidate(
  candidate: ParsedTaskCandidate,
  allowedProjects: ProjectInfo[],
) {
  return normalizeCanonicalProject(candidate, allowedProjects)
}
