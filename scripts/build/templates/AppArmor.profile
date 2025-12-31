# AppArmor 配置文件示例
# 用于限制 {{BINARY_NAME}} 容器的权限，防止容器逃逸
#
# 使用方法：
# 1. 将此文件保存到 /etc/apparmor.d/docker-{{BINARY_NAME}}
# 2. 重新加载：sudo apparmor_parser -r /etc/apparmor.d/docker-{{BINARY_NAME}}
# 3. 在 docker-compose.yml 中添加：security_opt: - apparmor:docker-{{BINARY_NAME}}

#include <tunables/global>

profile docker-{{BINARY_NAME}} flags=(attach_disconnected,mediate_deleted) {
  # 包含基础 Abstraction
  #include <abstractions/base>
  #include <abstractions/perl>

  # 允许基本 POSIX 操作
  capability chown,
  capability dac_override,
  capability setuid,
  capability setgid,
  capability fowner,
  capability fsetid,

  # 允许网络访问
  network inet stream,
  network inet dgram,
  network inet6 stream,
  network inet6 dgram,

  # 允许访问 /tmp（Runner 工作目录）
  /tmp/{{BINARY_NAME}}-workspace/** rw,
  /tmp/** rw,

  # 允许访问应用目录
  /app/** r,
  /app/migrations/** r,
  /app/config/** r,

  # 允许访问 Docker Socket（仅用于 Runner 管理）
  # 注意：这是高风险操作，生产环境建议使用专用隔离节点
  /var/run/docker.sock rw,

  # 允许读取系统库
  /etc/hosts r,
  /etc/resolv.conf r,
  /etc/ssl/certs/** r,
  /usr/lib/** r,
  /usr/lib/x86_64-linux-gnu/** r,

  # 拒绝访问敏感路径
  deny /root/** rw,
  deny /home/** rw,
  deny /var/log/** rw,
  deny /var/spool/** rw,
  deny /etc/shadow rw,
  deny /etc/passwd rw,

  # 禁止特权操作
  deny capability sys_admin,
  deny capability sys_module,
  deny capability sys_rawio,
  deny capability sys_ptrace,
  deny capability sys_boot,

  # 拒绝访问其他进程
  deny /proc/** rw,
  deny /sys/** rw,

  # 允许信号量操作
  deny ptrace (read, trace),

  # 允许基本的文件操作
  /bin/** rix,
  /usr/bin/** rix,
  /lib/** rix,
  /lib64/** rix,
}
