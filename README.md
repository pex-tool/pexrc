# Pexrc

[![Github Actions CI](https://github.com/pex-tool/pexrc/actions/workflows/ci.yml/badge.svg)](
https://github.com/pex-tool/pexrc/actions/workflows/ci.yml)

A native runtime bootstrap for PEXes.

PEXes must meet a few basic criteria to meet the historic Pex design goals:
+ Support both CPython 2.7 and PyPy 2.7 as well as 3.5 and up for both implementations.
+ Support multi-platform PEXes that include platform-specific wheels for each targeted platform.
+ Provide a hermetic runtime environment by default. You can only import the project packages your
  PEX ships with no matter the vagaries of the machines the PEX lands on.

Pexrc provides a `pexrc` binary that can take an existing zipapp PEX and replace its runtime
`.bootstrap/` with a native code bootstrap that meets all the design goals above while also
producing PEXes that are faster to execute in both cold and warm cache scenarios across the full
range of PEX sizes.

For example:
```console
# Given both a traditional zipapp PEX and a venv PEX:
:; pex cowsay -c cowsay -o cowsay.zipapp.pex
:; pex cowsay -c cowsay --venv -o cowsay.venv.pex

# Inject them with the new runtime:
:; time target/release/pexrc inject cowsay.zipapp.pex
Writing clibs/x86_64-linux-gnu.libpexrc.so 1407282 bytes to __pex__/.clibs/x86_64-linux-gnu.libpexrc.so...done.
Writing proxies/python-proxy-x86_64-linux-gnu 37472 bytes to __pex__/.proxies/python-proxy-x86_64-linux-gnu...done.

real	0m0.061s
user	0m0.058s
sys	0m0.004s

# Compare PEX sizes:
:; ls -1sh cowsay.zipapp.*
884K cowsay.zipapp.pex
1.9M cowsay.zipapp.pexrc

# Compare cold cache speed:
:; PEX_ROOT=/tmp/pex-cache hyperfine -w2 \
  -p 'rm -rf /tmp/pex-cache' \
  '/usr/bin/python3.14 cowsay.zipapp.pex -t Moo!' \
  '/usr/bin/python3.14 cowsay.venv.pex -t Moo!' \
  '/usr/bin/python3.14 cowsay.zipapp.pexrc -t Moo!'
Benchmark 1: /usr/bin/python3.14 cowsay.zipapp.pex -t Moo!
  Time (mean ± σ):     698.5 ms ±   6.5 ms    [User: 638.1 ms, System: 60.3 ms]
  Range (min … max):   689.5 ms … 706.8 ms    10 runs
 
Benchmark 2: /usr/bin/python3.14 cowsay.venv.pex -t Moo!
  Time (mean ± σ):     854.8 ms ±   4.8 ms    [User: 775.7 ms, System: 79.0 ms]
  Range (min … max):   848.4 ms … 862.2 ms    10 runs
 
Benchmark 3: /usr/bin/python3.14 cowsay.zipapp.pexrc -t Moo!
  Time (mean ± σ):     111.8 ms ±   1.1 ms    [User: 94.4 ms, System: 21.8 ms]
  Range (min … max):   109.8 ms … 114.8 ms    26 runs
 
Summary
  /usr/bin/python3.14 cowsay.zipapp.pexrc -t Moo! ran
    6.25 ± 0.08 times faster than /usr/bin/python3.14 cowsay.zipapp.pex -t Moo!
    7.64 ± 0.08 times faster than /usr/bin/python3.14 cowsay.venv.pex -t Moo!

# Compare warm cache speed:
:; PEX_ROOT=/tmp/pex-cache hyperfine -w2 \
  -s 'rm -rf /tmp/pex-cache' \
  '/usr/bin/python3.14 cowsay.zipapp.pex -t Moo!' \
  '/usr/bin/python3.14 cowsay.venv.pex -t Moo!' \
  '/usr/bin/python3.14 cowsay.zipapp.pexrc -t Moo!'
Benchmark 1: /usr/bin/python3.14 cowsay.zipapp.pex -t Moo!
  Time (mean ± σ):     293.9 ms ±   2.6 ms    [User: 262.3 ms, System: 31.5 ms]
  Range (min … max):   289.5 ms … 296.9 ms    10 runs
 
Benchmark 2: /usr/bin/python3.14 cowsay.venv.pex -t Moo!
  Time (mean ± σ):     106.3 ms ±   1.5 ms    [User: 92.0 ms, System: 14.1 ms]
  Range (min … max):   103.8 ms … 109.2 ms    28 runs
 
Benchmark 3: /usr/bin/python3.14 cowsay.zipapp.pexrc -t Moo!
  Time (mean ± σ):      64.5 ms ±   1.1 ms    [User: 52.8 ms, System: 11.5 ms]
  Range (min … max):    63.1 ms …  67.6 ms    46 runs
 
Summary
  /usr/bin/python3.14 cowsay.zipapp.pexrc -t Moo! ran
    1.65 ± 0.04 times faster than /usr/bin/python3.14 cowsay.venv.pex -t Moo!
    4.55 ± 0.08 times faster than /usr/bin/python3.14 cowsay.zipapp.pex -t Moo!
```

On the huge PEX side of the spectrum, some extra tricks come to the fore. Namely, injected PEXes use
zstd compression for ~all files except `__main__.py` and `PEX-INFO` and zip extraction is further
parallelized across all available cores.

Using the torch case:
```console
# Given a traditional zipapp torch PEX:
:; pex torch -o torch.pex

# Inject the PEX with the new runtime:
:; time target/release/pexrc inject torch.pex 
Writing clibs/x86_64-linux-gnu.libpexrc.so 1407282 bytes to __pex__/.clibs/x86_64-linux-gnu.libpexrc.so...done.
Writing proxies/python-proxy-x86_64-linux-gnu 37472 bytes to __pex__/.proxies/python-proxy-x86_64-linux-gnu...done.

real	0m7.925s
user	0m22.517s
sys	0m3.325s

# That took a little bit! But a modest space savings is a result:
:; ls -1sh torch.pex torch.pexrc
2.6G torch.pex
2.4G torch.pexrc

# Cold cache perf is improved:
:; PEX_ROOT=/tmp/pex-cache hyperfine -w1 -r3 \
  -p 'rm -rf /tmp/pex-cache' \
  '/usr/bin/python3.14 torch.pexrc -c "import torch; print(torch.__file__)"' \
  '/usr/bin/python3.14 torch.pex -c "import torch; print(torch.__file__)"'
Benchmark 1: /usr/bin/python3.14 torch.pexrc -c "import torch; print(torch.__file__)"
  Time (mean ± σ):      3.479 s ±  0.067 s    [User: 8.519 s, System: 2.769 s]
  Range (min … max):    3.411 s …  3.545 s    3 runs
 
Benchmark 2: /usr/bin/python3.14 torch.pex -c "import torch; print(torch.__file__)"
  Time (mean ± σ):     13.207 s ±  0.063 s    [User: 11.836 s, System: 1.367 s]
  Range (min … max):   13.150 s … 13.275 s    3 runs
 
Summary
  /usr/bin/python3.14 torch.pexrc -c "import torch; print(torch.__file__)" ran
    3.80 ± 0.08 times faster than /usr/bin/python3.14 torch.pex -c "import torch; print(torch.__file__)"

# As is warm cache perf:
:; PEX_ROOT=/tmp/pex-cache hyperfine -w1 -r3 \
  -s 'rm -rf /tmp/pex-cache' \
  '/usr/bin/python3.14 torch.pexrc -c "import torch; print(torch.__file__)"' \
  '/usr/bin/python3.14 torch.pex -c "import torch; print(torch.__file__)"'
Benchmark 1: /usr/bin/python3.14 torch.pexrc -c "import torch; print(torch.__file__)"
  Time (mean ± σ):     899.0 ms ±  11.1 ms    [User: 787.9 ms, System: 109.6 ms]
  Range (min … max):   891.9 ms … 911.8 ms    3 runs
 
Benchmark 2: /usr/bin/python3.14 torch.pex -c "import torch; print(torch.__file__)"
  Time (mean ± σ):      1.595 s ±  0.012 s    [User: 1.446 s, System: 0.148 s]
  Range (min … max):    1.582 s …  1.605 s    3 runs
 
Summary
  /usr/bin/python3.14 torch.pexrc -c "import torch; print(torch.__file__)" ran
    1.77 ± 0.03 times faster than /usr/bin/python3.14 torch.pex -c "import torch; print(torch.__file__)"
```

N.B.: You can now take advantage of these performance improvements in Pex 2.95.0 and later by adding
the `--rc` flag to your Pex command line.
