let println: any = 
    rec f: x: (() | (any, any)) |-> 
        match x
            | () => ()
            | (head: any, tail: any) => {
                discard print(head);
                f(tail)
            }
            | panic;
discard println("Hello, World!\n");
discard print 'A';
