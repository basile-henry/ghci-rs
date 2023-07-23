# ghci

 A crate to manage and communicate with `ghci` sessions

 ```rust
 let mut ghci = Ghci::new().unwrap();
 let out = ghci.eval("putStrLn \"Hello world\"").unwrap();
 assert_eq!(&out.stdout, "Hello world\n");
 ```

## License

[MIT License](./LICENSE)

Copyright 2023 Basile Henry
