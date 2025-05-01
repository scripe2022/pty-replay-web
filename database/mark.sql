DROP table IF EXISTS `marks`;

CREATE TABLE marks (
  id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  cast_id     BIGINT UNSIGNED NOT NULL,
  second      DOUBLE          NOT NULL,
  note        TEXT            NOT NULL DEFAULT 'mark',
  PRIMARY KEY (id),
  CONSTRAINT fk_marks_cast
    FOREIGN KEY (cast_id)
    REFERENCES casts(id)
    ON DELETE CASCADE
) ENGINE=InnoDB;
