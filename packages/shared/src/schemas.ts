import { z } from 'zod'

export const prioritySchema = z.enum(['high', 'medium', 'low'])
export const statusSchema = z.enum(['open', 'closed'])
export const taskSourceSchema = z.enum(['cli', 'web'])

export const taskSchema = z.object({
  id: z.string().trim().min(1),
  project: z.string().trim().min(1),
  priority: prioritySchema,
  status: statusSchema,
  description: z.string().trim().min(1),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime(),
  source: taskSourceSchema.optional(),
})

export const taskUpdateInputSchema = z.object({
  description: z.string().trim().min(1).optional(),
  priority: prioritySchema.optional(),
  status: statusSchema.optional(),
}).refine(
  (value) => value.description !== undefined || value.priority !== undefined || value.status !== undefined,
  'At least one mutable field must be provided.',
)

export const taskCreateInputSchema = z.object({
  project: z.string().trim().min(1),
  priority: prioritySchema,
  description: z.string().trim().min(1),
  source: taskSourceSchema.optional(),
})

export const projectInfoSchema = z.object({
  canonicalName: z.string().trim().min(1),
  path: z.string().trim().min(1),
  aliases: z.array(z.string().trim().min(1)),
})

export const trackAiConfigSchema = z.object({
  provider: z.enum(['openai', 'llama-cpp']).default('openai'),
  openai: z.object({
    model: z.string().trim().min(1).optional(),
  }).optional(),
  llamaCpp: z.object({
    modelPath: z.string().trim().min(1),
    llamaCliPath: z.string().trim().min(1).optional(),
    binaryPath: z.string().trim().min(1).optional(),
  }).optional(),
}).superRefine((value, context) => {
  if (value.provider === 'llama-cpp' && !value.llamaCpp?.modelPath) {
    context.addIssue({
      code: z.ZodIssueCode.custom,
      path: ['llamaCpp', 'modelPath'],
      message: 'llamaCpp.modelPath is required when ai.provider is llama-cpp.',
    })
  }
})

export const trackConfigSchema = z.object({
  projectRoots: z.array(z.string().trim().min(1)).default([]),
  projectAliases: z.record(z.string().trim().min(1)).default({}),
  ai: trackAiConfigSchema.default({
    provider: 'openai',
    openai: {},
  }),
})

export const parsedTaskCandidateSchema = z.object({
  project: z.string().trim().min(1).nullable(),
  priority: prioritySchema,
  description: z.string().trim().min(1),
  confidence: z.enum(['high', 'low']),
  reason: z.string().trim().min(1).optional(),
})

export const healthResponseSchema = z.object({
  ok: z.literal(true),
})

export const projectsResponseSchema = z.object({
  projects: z.array(projectInfoSchema),
})

export const tasksResponseSchema = z.object({
  tasks: z.array(taskSchema),
})

export const deleteTaskResponseSchema = z.object({
  ok: z.literal(true),
})

export const apiErrorResponseSchema = z.object({
  error: z.object({
    code: z.string().trim().min(1),
    message: z.string().trim().min(1),
  }),
})

export type PrioritySchema = z.infer<typeof prioritySchema>
export type StatusSchema = z.infer<typeof statusSchema>
export type TaskSchema = z.infer<typeof taskSchema>
export type TaskUpdateInputSchema = z.infer<typeof taskUpdateInputSchema>
export type TaskCreateInputSchema = z.infer<typeof taskCreateInputSchema>
export type ProjectInfoSchema = z.infer<typeof projectInfoSchema>
export type TrackAiConfigSchema = z.infer<typeof trackAiConfigSchema>
export type TrackConfigSchema = z.infer<typeof trackConfigSchema>
export type ParsedTaskCandidateSchema = z.infer<typeof parsedTaskCandidateSchema>
