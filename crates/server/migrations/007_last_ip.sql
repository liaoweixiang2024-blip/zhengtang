-- agent 最近一次连接的对端 IP（地区自动识别用）
ALTER TABLE servers ADD COLUMN last_ip TEXT;
