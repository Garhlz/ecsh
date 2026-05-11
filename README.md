二：编程部分：
1.尝试自行设计一个C语言小程序，完成最基本的shell角色：给出命令行提示符、能够逐次接受命令；对于命令分成三种，内部命令（例如help命令、exit命令等）、外部命令（常见的ls、cp等，以及其他磁盘上的可执行程序HelloWrold等）以及无效命令（命令输入错误），每次命令执行开始前shell输出“命令名称+starting…”，命令结束后shell输出“命令名称+ending.”。（20分）

2.将上述shell进行扩展，使得你编写的shell程序具有支持管道的功能，也就是说你的shell中输入“dir || more”能够执行dir命令并将其输出通过管道将其输入传送给more作为标准输入，同时要求在管道传输前shell输出“管道名称+ transferring data…”, 管道传输完毕shell输出“管道名称+ finish data.”。（10分）

我想使用rust + nix crate完成这个简单的实验。
