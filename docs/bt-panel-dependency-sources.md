# 宝塔面板 PHP 依赖下载来源分析

## 宝塔的混合下载策略

宝塔面板并非把所有依赖都放在自己服务器上，而是采用**混合策略**：

| 类型 | 来源 | 说明 |
|------|------|------|
| **系统依赖库**（libxml2-devel、openssl-devel 等） | **系统包管理器**（yum/apt） | 直接从 CentOS/Ubuntu 官方仓库安装，宝塔不提供 |
| **PHP/Nginx/Apache 源码** | **宝塔自己的 CDN**（`download.bt.cn`） | 从 php.net/nginx.org 等下载后重新托管，国内速度快 |
| **PECL 扩展**（redis、swoole 等） | **pecl.php.net** | 用 `pecl install` 命令从官方源安装 |
| **编译脚本本身** | **宝塔面板服务器** | 用户点"安装"时从宝塔服务器拉取对应的编译脚本 |

## 为什么宝塔不在自己服务器上提供依赖库？

1. **没必要** — 系统依赖库走 yum/apt 就行，这些都是系统自带的仓库
2. **版本太杂** — CentOS 7/8/9、Ubuntu 18/20/22/24、Debian 10/11/12 每个系统的包都不同，维护成本巨大
3. **依赖链复杂** — 一个 `libxml2-devel` 可能依赖 10 个子包，打包维护不现实

## 宝塔真正做的事情

1. **镜像源码包** — 把 `php-8.3.15.tar.gz` 这种源码包放在自己的 CDN 上（解决国内访问 php.net 慢的问题）
2. **启用正确的仓库** — 比如 CentOS 上先装 `epel-release`、启用 PowerTools/CRB
3. **编写稳健的编译脚本** — 动态检测、逐包安装、缺库跳过

## 缺少库的影响分级

### 致命级（缺了 PHP 编译成功但跑不了主流应用）

| 库 | 对应 PHP 扩展 | 影响 |
|---|---|---|
| `oniguruma-devel` | **mbstring** | WordPress、Laravel、ThinkPHP 等几乎所有现代 PHP 框架都依赖 `mb_*` 函数 |
| `libzip-devel` | **zip** | WordPress 插件安装/更新、Composer、phpMyAdmin 都需要 `ZipArchive` 类 |
| `libxml2-devel` | **libxml / DOM / SimpleXML** | 所有 XML 操作都依赖 |

### 重要级（缺了部分功能不可用）

| 库 | 对应 PHP 扩展 | 影响 |
|---|---|---|
| `openssl-devel` | openssl | HTTPS 请求、SSL 证书验证、`password_hash` |
| `libcurl-devel` | curl | HTTP 客户端（Guzzle、cURL 函数）不可用 |
| `libicu-devel` | intl | Laravel 等框架的国际化、货币格式化 |
| `libpng/jpeg/freetype` | gd | 图片处理（验证码、缩略图、头像裁剪） |
| `libmemcached-devel` | memcached (PECL) | Memcached 缓存，可改用 Redis 替代 |

### 可选级（缺了不影响大多数场景）

| 库 | 对应 PHP 扩展 | 影响 |
|---|---|---|
| `libsodium-devel` | sodium | 加密相关，老项目不用 |
| `libxslt-devel` | xsl | XSLT 模板转换，很少用 |
| `enchant2-devel` | enchant | 拼写检查，几乎没人用 |
| `gmp-devel` | gmp | 大数运算，除非做密码学 |
| `libtidy-devel` | tidy | HTML 清理，很少用 |
| `libwebp-devel` | webp | WebP 图片格式支持 |

## LeePanel 当前实现

### 下载源

当前从 **php.net 官网**下载源码：

```
https://www.php.net/distributions/php-8.3.15.tar.gz
```

国内服务器下载可能较慢。如需优化可考虑添加国内镜像源作为备用。

### 动态检测机制

已实现宝塔风格的动态检测：

- **逐包安装**：每个依赖独立安装，失败不阻塞
- **CRB 仓库自动启用**：CentOS 9 上自动启用 `crb`/`powertools`
- **库可用性检测**：`check_lib` 函数检测头文件 + pkg-config
- **动态 configure**：根据检测结果构建参数，缺库跳过对应扩展
- **PECL 容错**：每个扩展独立安装，失败记录到 `SKIPPED`
