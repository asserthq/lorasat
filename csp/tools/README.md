# libcsp reference vectors

`libcsp_v2_vectors.c` generates CSP v2 header fixtures by calling the real
`csp_id_prepend` function from libcsp v2.1 commit
`48f7fb0b57f610bf65bab1aa2d1357c3b9722782`.

Example from the `csp/` directory using a separate libcsp checkout and build
directory:

```sh
git clone --branch v2.1 --depth 1 https://github.com/libcsp/libcsp.git /tmp/libcsp-v2.1
cmake -S /tmp/libcsp-v2.1 -B /tmp/libcsp-build -DBUILD_SHARED_LIBS=OFF -DCSP_BUILD_SAMPLES=OFF
cmake --build /tmp/libcsp-build
cc -std=c11 \
  -I/tmp/libcsp-v2.1/include \
  -I/tmp/libcsp-build/include \
  tools/libcsp_v2_vectors.c \
  /tmp/libcsp-build/libcsp.a \
  -pthread \
  -o /tmp/libcsp-v2-vectors
/tmp/libcsp-v2-vectors
```

The checkout is not a project dependency. It is only used to reproduce test
fixtures.
