-- 机器档案：公网 IPv4/IPv6 与商家备注（卡片与详情页展示；IPv4 留空时前端回退 agent 连接 IP）
ALTER TABLE servers ADD COLUMN ipv4 TEXT;
ALTER TABLE servers ADD COLUMN ipv6 TEXT;
ALTER TABLE servers ADD COLUMN remark TEXT;
