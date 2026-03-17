# Pexrc

[![Github Actions CI](https://github.com/pex-tool/pexrc/actions/workflows/ci.yml/badge.svg)](
https://github.com/pex-tool/pexrc/actions/workflows/ci.yml)

A native runtime bootstrap for PEXes.

> [!WARNING]
> This set of tools is very alpha and definitely not intended for production use yet!

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
Writing x86_64-unknown-linux-gnu.libpexrc.so 1068848 bytes to __pex__/.clib/x86_64-unknown-linux-gnu.libpexrc.so...done.

real	0m0.056s
user	0m0.050s
sys	0m0.005s
:; time target/release/pexrc inject cowsay.venv.pex
Writing x86_64-unknown-linux-gnu.libpexrc.so 1068848 bytes to __pex__/.clib/x86_64-unknown-linux-gnu.libpexrc.so...done.

# Compare PEX sizes:
:; ls -1sh cowsay.zipapp.* cowsay.venv.*
880K cowsay.venv.pex
1.4M cowsay.venv.pexrc
860K cowsay.zipapp.pex
1.4M cowsay.zipapp.pexrc

# Compare cold cache speed:
:; hyperfine -w2 \
-p 'rm -rf ~/.cache/pex' 'python cowsay.zipapp.pex -t Moo!' \
-p 'rm -rf ~/.cache/pex' 'python cowsay.venv.pex -t Moo!' \
-p 'rm -rf ~/.cache/pexrc/' 'python cowsay.zipapp.pexrc -t Moo!' \
-p 'rm -rf ~/.cache/pexrc/' 'python cowsay.venv.pexrc -t Moo!'
Benchmark 1: python cowsay.zipapp.pex -t Moo!
  Time (mean ± σ):     859.0 ms ±  10.6 ms    [User: 777.3 ms, System: 81.6 ms]
  Range (min … max):   846.4 ms … 885.2 ms    10 runs

Benchmark 2: python cowsay.venv.pex -t Moo!
  Time (mean ± σ):      1.030 s ±  0.016 s    [User: 0.920 s, System: 0.111 s]
  Range (min … max):    1.005 s …  1.055 s    10 runs

Benchmark 3: python cowsay.zipapp.pexrc -t Moo!
  Time (mean ± σ):     132.9 ms ±   1.7 ms    [User: 114.6 ms, System: 26.4 ms]
  Range (min … max):   130.3 ms … 137.1 ms    21 runs

Benchmark 4: python cowsay.venv.pexrc -t Moo!
  Time (mean ± σ):     134.2 ms ±   3.0 ms    [User: 116.0 ms, System: 26.4 ms]
  Range (min … max):   129.5 ms … 141.5 ms    22 runs

Summary
  python cowsay.zipapp.pexrc -t Moo! ran
    1.01 ± 0.03 times faster than python cowsay.venv.pexrc -t Moo!
    6.46 ± 0.12 times faster than python cowsay.zipapp.pex -t Moo!
    7.75 ± 0.15 times faster than python cowsay.venv.pex -t Moo!

# Compare warm cache speed:
:; hyperfine -w2 \
'python cowsay.zipapp.pex -t Moo!' \
'python cowsay.venv.pex -t Moo!' \
'python cowsay.zipapp.pexrc -t Moo!' \
'python cowsay.venv.pexrc -t Moo!'
Benchmark 1: python cowsay.zipapp.pex -t Moo!
  Time (mean ± σ):     362.1 ms ±  17.6 ms    [User: 323.7 ms, System: 38.6 ms]
  Range (min … max):   336.5 ms … 391.8 ms    10 runs

Benchmark 2: python cowsay.venv.pex -t Moo!
  Time (mean ± σ):     111.9 ms ±   6.0 ms    [User: 97.4 ms, System: 14.4 ms]
  Range (min … max):   102.5 ms … 131.4 ms    27 runs

Benchmark 3: python cowsay.zipapp.pexrc -t Moo!
  Time (mean ± σ):      71.8 ms ±   5.0 ms    [User: 58.9 ms, System: 12.9 ms]
  Range (min … max):    64.4 ms …  93.0 ms    39 runs

Benchmark 4: python cowsay.venv.pexrc -t Moo!
  Time (mean ± σ):      69.2 ms ±   4.8 ms    [User: 57.1 ms, System: 12.0 ms]
  Range (min … max):    63.7 ms …  84.1 ms    45 runs

Summary
  python cowsay.venv.pexrc -t Moo! ran
    1.04 ± 0.10 times faster than python cowsay.zipapp.pexrc -t Moo!
    1.62 ± 0.14 times faster than python cowsay.venv.pex -t Moo!
    5.23 ± 0.45 times faster than python cowsay.zipapp.pex -t Moo!
```

On the huge PEX side of the spectrum, some extra tricks come to the fore. Namely, injected PEXes use
zstd compression for all files except `__main__.py` and `PEX-INFO` and zip extraction is further
parallelized across all available cores.

Using the torch case:
```console
# Given a traditional zipapp torch PEX:
:; pex torch -o torch.pex

# Inject the PEX with the new runtime:
:; time target/release/pexrc inject torch.pex
Writing x86_64-unknown-linux-gnu.libpexrc.so 1068848 bytes to __pex__/.clib/x86_64-unknown-linux-gnu.libpexrc.so...done.

real	0m39.855s
user	0m38.108s
sys	0m1.579s

# That took a little bit! But a pretty big space savings is a result:
:; ls -1sh torch.pex torch.pexrc
3.9G torch.pex
3.2G torch.pexrc

# Cold cache perf is improved:
:; hyperfine -w1 -r3 \
-p 'rm -rf ~/.cache/pexrc' 'python torch.pexrc -c "import torch; print(torch.__file__)"' \
-p 'rm -rf ~/.cache/pex' 'python torch.pex -c "import torch; print(torch.__file__)"'
Benchmark 1: python torch.pexrc -c "import torch; print(torch.__file__)"
  Time (mean ± σ):      5.563 s ±  0.111 s    [User: 14.526 s, System: 3.968 s]
  Range (min … max):    5.455 s …  5.678 s    3 runs

Benchmark 2: python torch.pex -c "import torch; print(torch.__file__)"
  Time (mean ± σ):     29.046 s ±  0.131 s    [User: 27.196 s, System: 1.733 s]
  Range (min … max):   28.897 s … 29.145 s    3 runs

Summary
  python torch.pexrc -c "import torch; print(torch.__file__)" ran
    5.22 ± 0.11 times faster than python torch.pex -c "import torch; print(torch.__file__)"

# As is warm cache perf:
:; hyperfine -w1 -r3 \
'python torch.pexrc -c "import torch; print(torch.__file__)"' \
'python torch.pex -c "import torch; print(torch.__file__)"'
Benchmark 1: python torch.pexrc -c "import torch; print(torch.__file__)"
  Time (mean ± σ):      1.207 s ±  0.010 s    [User: 1.045 s, System: 0.160 s]
  Range (min … max):    1.195 s …  1.215 s    3 runs

Benchmark 2: python torch.pex -c "import torch; print(torch.__file__)"
  Time (mean ± σ):      1.975 s ±  0.013 s    [User: 1.807 s, System: 0.168 s]
  Range (min … max):    1.963 s …  1.988 s    3 runs

Summary
  python torch.pexrc -c "import torch; print(torch.__file__)" ran
    1.64 ± 0.02 times faster than python torch.pex -c "import torch; print(torch.__file__)"
```

N.B,.: The ideas developed in this repo, once proved out, will likely move into the main Pex repo or
at least used by the Pex CLI tool to replace the current pure-Python PEX bootstrap runtime.
