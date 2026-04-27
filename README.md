# track

`track` is a small issue tracker with three moving parts:

- a local CLI that captures tasks from rough notes
- a local WebUI for editing, dispatching, and reviewing
- a remote runner that can use Codex or Claude

## Disclaimer/preamble

It is a tool that I use myself and it's not really meant to be something
useful for other people. If it works for you, great. If it doesn't, well,
that's fine too. Not so much value for the world here in these wild times.

It was initially vibe coded for a ~month, as an experiment in vibe coding.
Surprise-surpise, the result codebase was a disaster. However, at the same
time the tool turned out to be very convenient:

a) I can write down tasks wherever and however I want and I stopped forgetting
things I wanted to do.

b) Stupidly simple tasks are fine to dispatch to a remote agent in a "fire and
forget" mode. When it's done, I'm pinged, and usually results are fine to
quickly review and merge. If not, it's fine to throw the PR away.

c) Existing solutions of that sort do not seem to be security-oriented. This
one kind of is. It assumes the remote environment as "anything can happen",
does basic protection against prompt injection, and tries to keep the host
safe. So I'm more confident in running this thing than OpenClaw or whatever.

So since I've started using this, I'm slowly un-vibing this codebase. The
project was flawed at the very beginning, so it takes more time than it should,
but I guess un-vibing codebases may prove a useful skill in the future, and
~~I must suffer for my sins~~ it's kind of a fun challenge, somehow.

Meanwhile, the code in this repository is not representative of what I typically
create. Use at your own risk. Although, as stated, I do use this tool myself,
and it does work well enough for me.

## Documentation

The canonical documentation is published at [https://popzxc.github.io/track/](https://popzxc.github.io/track/), and its source lives under [`docs/`](./docs/).

Start here:

- [Initial setup](https://popzxc.github.io/track/initial-setup/intro/)
- [Configuring projects and runner settings](https://popzxc.github.io/track/configuring/register-projects/)
- [Using the WebUI](https://popzxc.github.io/track/using-webui/dispatching-tasks/)
- [Reference](https://popzxc.github.io/track/reference/config-files/)
- [Development flow](https://popzxc.github.io/track/development-flow/development-flow/)

## Local docs development

```bash
just run-docs
```

## License

`track` is licensed under [MIT](./LICENSE).
