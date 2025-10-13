let println: any = 
    rec f: match
        | () => ()
        | (head: char, tail: any) => {
            discard print!(head);
            f(tail)
        }
        | panic;
discard println("Hello, World!\n");
