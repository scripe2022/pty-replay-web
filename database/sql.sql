DROP table IF EXISTS `casts`;
DROP table IF EXISTS `heartbeats`;
DROP table IF EXISTS `logs`;

CREATE TABLE logs (
  id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  uuid        UUID            NOT NULL,
  note        TEXT            NOT NULL DEFAULT '',
  uploaded_at TIMESTAMP(0)    NOT NULL DEFAULT CURRENT_TIMESTAMP(0),
  visible     BOOLEAN         NOT NULL DEFAULT TRUE,
  PRIMARY KEY (id),
  UNIQUE KEY uk_logs_uuid (uuid)
) ENGINE=InnoDB;

CREATE TABLE casts (
  id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  uuid        UUID            NOT NULL,
  bucket      TEXT            NOT NULL,
  path        TEXT            NOT NULL,
  size_byte   BIGINT UNSIGNED NOT NULL,
  started_at  TIMESTAMP(0)    NOT NULL,
  PRIMARY KEY (id),
  KEY idx_casts_uuid (uuid),
  CONSTRAINT fk_casts_log
    FOREIGN KEY (uuid)
    REFERENCES logs(uuid)
    ON DELETE CASCADE
) ENGINE=InnoDB;

CREATE TABLE heartbeats (
  id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  uuid        UUID            NOT NULL,
  session     INT UNSIGNED    NOT NULL,
  started_at  TIMESTAMP(0)    NOT NULL,
  ended_at    TIMESTAMP(0)    NOT NULL,
  PRIMARY KEY (id),
  KEY idx_hb_uuid (uuid),
  CONSTRAINT fk_hb_log
    FOREIGN KEY (uuid)
    REFERENCES logs(uuid)
    ON DELETE CASCADE,
  CHECK (ended_at >= started_at)
) ENGINE=InnoDB;

