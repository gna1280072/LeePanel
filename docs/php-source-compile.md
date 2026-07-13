# 从源码编译 PHP（宝塔方式）

## 1. 安装编译依赖

```bash
apt install build-essential autoconf pkg-config \
  libxml2-dev libssl-dev libcurl4-openssl-dev libsqlite3-dev \
  libpng-dev libjpeg-dev libfreetype6-dev libzip-dev \
  libonig-dev libsodium-dev libreadline-dev
```

## 2. 下载源码

```bash
wget https://www.php.net/distributions/php-8.3.20.tar.gz
tar xzf php-8.3.20.tar.gz
cd php-8.3.20
```

## 3. 配置（关键步骤）

```bash
./configure \
  --prefix=/www/server/php/8.3 \
  --with-config-file-path=/www/server/php/8.3/etc \
  --with-config-file-scan-dir=/www/server/php/8.3/etc/php.d \
  --enable-fpm \
  --with-fpm-user=www \
  --with-fpm-group=www \
  --with-mysqli \
  --with-pdo-mysql \
  --with-openssl \
  --with-curl \
  --with-zip \
  --with-gd \
  --with-jpeg \
  --with-freetype \
  --enable-mbstring \
  --enable-sockets \
  --enable-opcache
```

`--prefix` 指定所有东西装到哪个目录，这是宝塔能"一个目录搞定"的核心。

## 4. 编译安装

```bash
make -j$(nproc)
make install
```

编译时间取决于服务器性能，一般 3~10 分钟。

## 5. 配置

```bash
cp php.ini-production /www/server/php/8.3/etc/php.ini
cp /www/server/php/8.3/etc/php-fpm.conf.default /www/server/php/8.3/etc/php-fpm.conf
cp /www/server/php/8.3/etc/php-fpm.d/www.conf.default /www/server/php/8.3/etc/php-fpm.d/www.conf
```

## 6. 注册为系统服务

创建 `/etc/systemd/system/php83-fpm.service`：

```ini
[Unit]
Description=PHP 8.3 FPM
After=network.target

[Service]
ExecStart=/www/server/php/8.3/sbin/php-fpm --nodaemonize --fpm-config /www/server/php/8.3/etc/php-fpm.conf
ExecReload=/bin/kill -USR2 $MAINPID
Type=notify

[Install]
WantedBy=multi-user.target
```

```bash
systemctl daemon-reload
systemctl enable php83-fpm
systemctl start php83-fpm
```

## 要点总结

| 环节 | 关键 |
|------|------|
| 自包含目录 | `--prefix=/www/server/php/8.3` |
| 配置文件位置 | `--with-config-file-path` |
| 多版本共存 | 每个版本用不同 prefix 目录 |
| 扩展安装 | 用编译好的 `phpize` 和 `php-config` |

核心原理：标准的 `./configure && make && make install`，通过 `--prefix` 把所有东西收拢到一个目录下。
