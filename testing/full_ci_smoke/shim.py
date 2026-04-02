#!/usr/bin/env python3

import sys

from smoke_test.platform_shims import main


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
