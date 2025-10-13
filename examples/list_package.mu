let List: any = T: any |-> rec list: (() | (T, list));
let Nil: any = ();
List::List & Nil::Nil
