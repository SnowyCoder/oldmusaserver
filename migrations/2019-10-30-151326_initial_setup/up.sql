CREATE TABLE user_account (
	id SERIAL NOT NULL,
	username VARCHAR(32) NOT NULL UNIQUE,
	password_hash VARCHAR(128) NOT NULL,
	last_password_change TIMESTAMP NOT NULL,
	permission CHAR NOT NULL,
	PRIMARY KEY (id)
);

CREATE TABLE site (
	id SERIAL NOT NULL,
	name VARCHAR(100),
	id_cnr VARCHAR(50),
	PRIMARY KEY (id)
);

CREATE TABLE user_access (
	user_id SERIAL NOT NULL,
	site_id SERIAL NOT NULL,
	PRIMARY KEY (user_id, site_id),
	FOREIGN KEY(user_id) REFERENCES user_account (id) ON DELETE CASCADE,
	FOREIGN KEY(site_id) REFERENCES site (id) ON DELETE CASCADE
);

CREATE TABLE sensor (
	id SERIAL NOT NULL,
	site_id INTEGER NOT NULL,
	id_cnr VARCHAR(50),
	name VARCHAR(50),
	loc_x INTEGER,
	loc_y INTEGER,
	enabled BOOLEAN NOT NULL DEFAULT false,
	status VARCHAR(100) NOT NULL DEFAULT 'ok',
	PRIMARY KEY (id),
	FOREIGN KEY(site_id) REFERENCES site (id) ON DELETE CASCADE
);

CREATE TABLE fcm_user_contact (
	registration_id VARCHAR(255) NOT NULL,
	user_id INTEGER NOT NULL,
	PRIMARY KEY (registration_id),
	FOREIGN KEY(user_id) REFERENCES user_account (id) ON DELETE CASCADE
);

CREATE TABLE channel (
	id SERIAL NOT NULL,
	sensor_id SERIAL NOT NULL,
	id_cnr VARCHAR(50),
	name VARCHAR(50),
	measure_unit VARCHAR(50),
	range_min NUMERIC,
	range_max NUMERIC,
	PRIMARY KEY (id),
	FOREIGN KEY(sensor_id) REFERENCES sensor (id) ON DELETE CASCADE
);
