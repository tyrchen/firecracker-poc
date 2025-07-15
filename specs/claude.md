# Claude code instructions

this is a repo for a poc of building a sandbox env using firecracker. see
./sepcs/0001-poc.md for details. Now the basic functions are ready
however for code execution, there's no output. Please double check if the
code logic in ./src/runner.rs are correct and fix any issues you may
see.

Looks like this path is not viable. Could you switch to a different
solution that the VM created by firecracker contains a python API server
so that it could receive the code from http, run it and return the
result. With this, we don't need to hack on init. Please alter the
runner.rs code and provide a script to generate the proper rootfs that
contains the API server.

it works well! However the latency is pretty big. Can you optimize it to make the latency as small as possible?
