// 阶乘和斐波那契数列示例

// 普通递归阶乘
let factorial: any = 
    rec fact: match
        | 0 => 1
        | 1 => 1
        | n: any => n * fact(n - 1)
        | panic;

// 尾递归阶乘
let factorial_tail: any = n: int |-> [
        let helper: any = rec h: acc: int |-> match
            | 0 => acc
            | 1 => acc
            | n: any => h(acc * n)(n - 1)
            | panic;
        helper(1)(n)
    ];

// 斐波那契数列
let fibonacci: any = 
    rec fib: match
        | 0 => 0
        | 1 => 1
        | n: any => fib(n - 1) + fib(n - 2)
        | panic;

// 尾递归斐波那契
let fibonacci_tail: any = n: int |-> [
    let helper: any = rec helper: a: int |-> b: int |-> match
        | 0 => a
        | n: any => helper(b)(a + b)(n - 1)
        | panic;
        helper(0)(1)(n)
    ];

// 测试
factorial(5), factorial_tail(5), fibonacci(7), fibonacci_tail(7)
