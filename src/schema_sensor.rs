// Schema for the "sensor" database, that is the database from which to query the sensor data

table! {
    t_rilevamento_dati (data, idsito, idstanza, idstazione, idsensore, canale, misura) {
        idsito -> Varchar,
        idstanza -> Varchar,
        idstazione -> Varchar,
        idsensore -> Varchar,
        canale -> Varchar,
        valore_min -> Double,
        valore_med -> Nullable<Double>,
        valore_max -> Nullable<Double>,
        scarto -> Nullable<Double>,
        data -> Datetime,
        errore -> Nullable<Char>,
        misura -> Varchar,
        step -> Nullable<Float>,
    }
}

