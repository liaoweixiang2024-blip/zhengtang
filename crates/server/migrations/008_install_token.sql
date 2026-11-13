-- 一次性安装令牌（短安装命令）：/a/{token} 据此下发已注入参数的 install.sh
ALTER TABLE servers ADD COLUMN secret_key TEXT;            -- agent 私钥（生成安装令牌所需）
ALTER TABLE servers ADD COLUMN install_token TEXT;         -- 安装令牌（24h 限时）
ALTER TABLE servers ADD COLUMN install_token_expires INTEGER;
