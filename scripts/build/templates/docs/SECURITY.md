# {{BINARY_NAME}} 安全部署检查清单

本文档提供了部署 {{BINARY_NAME}} 及其 Runner 功能的安全检查清单。

## 📋 部署前检查

### 基础安全

- [ ] 运行在受信任的内网环境或 VPN 中
- [ ] 使用最新的稳定版本镜像
- [ ] 配置防火墙规则，仅允许必要的端口访问
- [ ] 启用 TLS/SSL 加密（生产环境）

### 容器安全

- [ ] 使用非 root 用户运行容器（默认：opsuser）
- [ ] 启用 `no-new-privileges` 安全选项
- [ ] 配置资源限制（CPU、内存）
- [ ] 使用只读根文件系统（可选）
- [ ] 不要使用 `--privileged` 标志

### Docker Socket 安全

- [ ] 以只读方式挂载 docker.sock（如果可能）
- [ ] 启用 AppArmor 或 SELinux 策略
- [ ] 配置审计日志监控 docker.sock 访问
- [ ] 定期审查 Docker API 调用日志

### Runner 工作目录隔离

- [ ] 设置 `RUNNER_WORK_DIR` 环境变量
- [ ] 设置 `RUNNER_WORKSPACE_PREFIX` 环境变量
- [ ] 确保清理策略只删除此前缀下的目录
- [ ] 定期检查工作目录，确保无残留文件

### 网络隔离

- [ ] 使用独立的 Docker 网络
- [ ] 固定网络子网（便于策略控制）
- [ ] Runner 容器加入相同的网络
- [ ] 限制容器间不必要的网络访问

### 资源限制

- [ ] 设置 CPU 限制（建议不超过宿主机的 50%）
- [ ] 设置内存限制（建议不超过宿主机的 50%）
- [ ] 监控容器资源使用情况
- [ ] 配置告警规则

## 🔒 高级安全加固

### AppArmor 配置

```bash
# 1. 复制配置文件
sudo cp security/AppArmor.profile /etc/apparmor.d/docker-{{BINARY_NAME}}

# 2. 加载配置
sudo apparmor_parser -r /etc/apparmor.d/docker-{{BINARY_NAME}}

# 3. 验证配置
sudo aa-status | grep {{BINARY_NAME}}

# 4. 在 docker-compose.yml 中启用
security_opt:
  - apparmor:docker-{{BINARY_NAME}}
```

### SELinux 配置（如果使用 CentOS/RHEL）

```bash
# 为容器创建 SELinux 策略
sudo semanage fcontext -a -t container_file_t "/var/lib/{{BINARY_NAME}}(/.*)?"
sudo restorecon -Rv /var/lib/{{BINARY_NAME}}
```

### 审计配置

```bash
# 启用 Docker 审计
sudo auditctl -w /var/run/docker.sock -p wa -k docker

# 启用容器文件操作审计
sudo auditctl -w /app/{{BINARY_NAME}} -p wa -k {{BINARY_NAME}}

# 查看审计日志
sudo ausearch -k docker
sudo ausearch -k {{BINARY_NAME}}
```

## 🚨 安全事件响应

### 检测异常行为

如果发现以下异常，立即调查：

- Runner 容器尝试访问工作目录外的路径
- 异常的 Docker API 调用（如删除非本容器的容器）
- 容器资源使用突然激增
- 未知的网络连接

### 应急措施

```bash
# 1. 立即停止容器
docker stop {{BINARY_NAME}}
docker-compose down

# 2. 检查日志
docker logs {{BINARY_NAME}} --tail 100

# 3. 审查创建的容器
docker ps -a | grep runner

# 4. 检查工作目录
ls -la /tmp/{{BINARY_NAME}}-workspace/

# 5. 检查审计日志
sudo ausearch -k docker -ts recent

# 6. 如果确认被入侵，保留现场并联系安全团队
```

## 📊 定期安全审查

建议定期执行以下检查：

### 每日

- [ ] 检查容器运行状态
- [ ] 查看应用日志是否有异常
- [ ] 监控资源使用情况

### 每周

- [ ] 审查 Docker API 调用日志
- [ ] 检查创建的 Runner 容器列表
- [ ] 清理残留的 Runner 容器和镜像

### 每月

- [ ] 更新基础镜像到最新版本
- [ ] 审查安全策略（AppArmor/SELinux）
- [ ] 测试备份恢复流程
- [ ] 进行安全渗透测试

## 🔐 密钥和凭证管理

- [ ] 使用强密码（至少 16 位，包含大小写字母、数字、特殊字符）
- [ ] 定期轮换数据库密码
- [ ] 使用 Docker secrets 或环境变量管理敏感信息
- [ ] 不要在日志中输出敏感信息
- [ ] 启用数据库 SSL 连接

## 🌐 网络安全

- [ ] 使用反向代理（如 Nginx）并启用 HTTPS
- [ ] 配置防火墙规则
- [ ] 启用请求速率限制
- [ ] 使用 WAF（Web 应用防火墙）
- [ ] 定期更新 TLS 证书

## 💾 数据备份

- [ ] 每日备份数据库
- [ ] 备份配置文件
- [ ] 测试恢复流程
- [ ] 备份存储在异地
- [ ] 加密备份数据

## 📚 参考资源

- [Docker 安全最佳实践](https://docs.docker.com/engine/security/)
- [AppArmor 文档](https://gitlab.com/apparmor/apparmor/-/wikis/home/)
- [Linux Capabilities](https://man7.org/linux/man-pages/man7/capabilities.7.html)
- [容器安全检查清单](https://snyk.io/blog/10-docker-image-security-best-practices/)

---

**最后更新**: {{VERSION}}
**文档维护者**: {{BINARY_NAME}} Team
