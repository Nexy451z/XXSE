/*
 * Route B entry point: devkitA64's gcc links this file together with the Rust
 * staticlib (libmhnx_switch.a) that cargo build produces for the custom target.
 * This is the "devkitPro Makefile" way of consuming the Rust code and is what
 * build.ps1 -Route c uses.
 */
extern int mhnx_main(void);

int main(int argc, char** argv)
{
    (void)argc;
    (void)argv;
    return mhnx_main();
}
