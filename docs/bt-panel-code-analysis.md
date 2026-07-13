# 宝塔面板 PHP 编译脚本为何有 3000 行

## 1. 支持 PHP 5.2 ~ 8.4（约 15 个大版本）

每个大版本的 configure 参数都不同：

```bash
# PHP 5.2 — 已废弃的参数
--with-mysql=mysqlnd --with-mysqli=mysqlnd --enable-zend-multibyte

# PHP 5.6 — 还有 mysql 扩展
--with-mysql --with-mysqli --with-pdo-mysql

# PHP 7.0 — 移除了 mysql，新增了 sodium
--with-mysqli --with-pdo-mysql --with-sodium

# PHP 8.0 — GD 参数全改，移除了 --with-gd
--enable-gd --with-jpeg --with-freetype

# PHP 8.1 — 新增 avif
--with-avif
```

宝塔为**每个 PHP 版本**写了独立的编译函数，光这部分就 ~1500 行。

## 2. 支持 6+ 种 Linux 发行版

```bash
# 每种系统一个函数
install_centos_deps()     # ~100 行
install_ubuntu_deps()     # ~100 行
install_debian_deps()     # ~100 行
install_alma_deps()       # ~100 行
install_rocky_deps()      # ~100 行
install_openeuler_deps()  # ~100 行
```

每个系统的包名、仓库启用方式、默认路径都不同。

## 3. 每个 PECL 扩展都有独立逻辑（~800 行）

```bash
# redis — 需要指定版本兼容 PHP 7.4 vs 8.x
install_redis_ext() {
  if [ "$PHP_VER" = "5.6" ]; then
    pecl install redis-4.3.0  # 最后支持 5.6 的版本
  elif [ "$PHP_VER" = "7.4" ]; then
    pecl install redis-5.3.7
  else
    pecl install redis
  fi
}

# imagick — 需要先装 ImageMagick 源码编译
install_imagick_ext() {
  if ! command -v convert &>/dev/null; then
    # 下载编译 ImageMagick 源码 ~50 行
    wget https://...
    ./configure --prefix=/usr/local/imagemagick
    make && make install
  fi
  pecl install imagick
}

# swoole — 需要指定版本 + 编译参数
install_swoole_ext() {
  pecl install swoole-5.1.0  # 固定版本避免兼容问题
}

# memcached — 需要先装 libmemcached
install_memcached_ext() {
  if ! pkg-config --exists libmemcached; then
    # 从源码编译 libmemcached ~30 行
  fi
  pecl install memcached
}
```

每个扩展 50-100 行，5-8 个扩展就是 400-800 行。

## 4. 大量的兼容性处理和兜底（~500 行）

```bash
# OpenSSL 版本检测（3.0 vs 1.1 参数不同）
if openssl version | grep -q "3\."; then
  CF="$CF --with-openssl"
else
  CF="$CF --with-openssl --with-openssl-dir=/usr"
fi

# libzip 版本太低需要源码编译
if ! pkg-config --atleast-version=0.11 libzip; then
  # 下载编译 libzip 源码 ~30 行
fi

# PHP 7.4 + CentOS 7 的 oniguruma 需要从源码编译
if [ "$PHP_VER" = "7.4" ] && [ "$OS_ID" = "centos" ] && [ "$OS_VER" = "7" ]; then
  # 源码编译 oniguruma ~20 行
fi

# 各种历史 bug 的修复
# 比如 PHP 7.2 在 GCC 10+ 上编译报错的 workaround
# 比如 PHP 5.6 在 OpenSSL 3.0 上无法编译的补丁
```

## 5. 日志、进度、回滚（~300 行）

```bash
# 宝塔面板需要实时回传进度给前端
echo "正在安装依赖..." > /tmp/panelExec.log
echo "15" > /tmp/panelProgress.pl  # 进度百分比

# 安装失败需要清理
cleanup_on_failure() {
  rm -rf "$PHP_PREFIX"
  rm -rf "$PHP_SRC_DIR"
  systemctl daemon-reload
}
```

## 对比总结

| 模块 | 宝塔行数 | TaiChi Panel 行数 | 差异原因 |
|------|---------|---------|---------|
| PHP 版本支持 | ~1500 | ~20 | 宝塔支持 5.2-8.4（15个版本），我们 7.4-8.4（共用一套动态逻辑） |
| 多系统依赖 | ~600 | ~50 | 宝塔 6 个独立函数，我们用动态检测统一处理 |
| PECL 扩展 | ~800 | ~30 | 宝塔每个扩展独立版本管理+源码编译依赖 |
| 兼容性兜底 | ~500 | ~30 | 宝塔积累了 10 年的 bug 修复 |
| 日志/进度 | ~300 | ~10 | 宝塔需要实时回传面板前端 |
| **合计** | **~3000** | **~300** | — |

## 为什么我们能少 10 倍？

1. **不支持 PHP 5.x / 8.0 以下** — 直接砍掉一半版本适配代码
2. **动态检测** — 一套逻辑通吃所有系统，不需要每个系统单独写
3. **PECL 简化** — 用 `pecl install` 最新版，不做版本固定
4. **无进度回传** — 我们走 SSH 终端直出，不需要写文件传进度

宝塔 3000 行里大约一半是**历史包袱**（PHP 5.x、老系统兼容），另一半是真正有价值的**边界情况处理**。我们的 300 行动态检测已经覆盖了最核心的部分。随着实际运行中遇到更多边界情况，代码量会逐步增长，但不需要追到 3000 行。
