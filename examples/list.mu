// 链表操作示例
let Cons: any = head: any |-> tail: any |-> Cons::(head, tail);
let Nil: any = Nil::();

// 列表长度
let length: any = rec len: match
    | Nil::() => 0
    | Cons::(any, tail: any) => 1 + len(tail)
    | panic;

// 列表求和
let sum: any = rec s: match
    | Nil::() => 0
    | Cons::(head: int, tail: any) => head + s(tail)
    | panic;

// 列表映射
let map_list: any = 
    rec m: list: (Nil::() | Cons::(any, any)) |-> f: any |->
        match list
            | Nil::() => Nil
            | Cons::(head: any, tail: any) => Cons(f(head))(m(tail)(f))
            | panic;

// 创建示例列表: [1, 2, 3, 4, 5]
let mylist: any = Cons(1)(Cons(2)(Cons(3)(Cons(4)(Cons(5)(Nil)))));

// 测试
length(mylist),
sum(mylist),
sum(map_list(mylist)(x: int |-> x * 2))
