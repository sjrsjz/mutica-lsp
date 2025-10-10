let println: any = 
    rec f: x: (() | (char, any)) |-> 
        match x
            | () => ()
            | (head: char, tail: any) => {
                discard print(head);
                f(tail)
            }
            | panic;
discard println("Hello, World!\n");
discard print 'A';
