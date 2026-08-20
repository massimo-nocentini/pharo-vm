# Plugin skeleton

Copy this directory somewhere outside the Pharo tree, rename the two
`.template` files, and replace `MyPlugin` throughout:

```sh
cp -r rust/pharo-vm-plugin/template ~/my-pharo-plugin
cd ~/my-pharo-plugin
mv Cargo.toml.template Cargo.toml
mv src/lib.rs.template src/lib.rs
# then edit both, replacing MyPlugin with your module name
cargo build --release
cp target/release/libMyPlugin.so /path/to/pharo/directory/
```

The files carry a `.template` suffix so cargo does not treat this directory as
a crate while it sits inside the Pharo workspace.

See ../README.md for the full guide.
