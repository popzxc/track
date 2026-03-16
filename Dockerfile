FROM oven/bun:1.2 AS base
WORKDIR /app

COPY package.json tsconfig.base.json ./
COPY apps/api/package.json apps/api/package.json
COPY apps/cli/package.json apps/cli/package.json
COPY apps/web/package.json apps/web/package.json
COPY packages/shared/package.json packages/shared/package.json
COPY packages/core/package.json packages/core/package.json

RUN bun install

FROM base AS build
COPY . .
RUN bun run build:shared
RUN bun run build:core
RUN bun run build:web
RUN bun run build:api

FROM oven/bun:1.2-slim AS runtime
WORKDIR /app

ENV NODE_ENV=production
ENV PORT=3210

COPY --from=build /app/package.json ./package.json
COPY --from=build /app/node_modules ./node_modules
COPY --from=build /app/apps/api/dist ./apps/api/dist
COPY --from=build /app/apps/api/package.json ./apps/api/package.json
COPY --from=build /app/apps/web/dist ./apps/api/public
COPY --from=build /app/packages/shared/package.json ./packages/shared/package.json
COPY --from=build /app/packages/shared/dist ./packages/shared/dist
COPY --from=build /app/packages/core/package.json ./packages/core/package.json
COPY --from=build /app/packages/core/dist ./packages/core/dist

EXPOSE 3210

CMD ["bun", "apps/api/dist/server.js"]
