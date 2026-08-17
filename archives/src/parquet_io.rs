//! Snappy-compressed Parquet encoding for any `RecordBatch`.
//!
//! Lifted out of the original `person` module in Commit 2 so the
//! generic snapshot driver can call it for every table.

use anyhow::{Context, Result};
use arrow::array::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

/// Encode `batch` as a Snappy-compressed Parquet file in memory.
pub fn encode_parquet(batch: &RecordBatch) -> Result<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props))
        .context("create arrow parquet writer")?;
    writer.write(batch).context("write record batch")?;
    writer.close().context("close parquet writer")?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::encode_parquet;
    use crate::generic::batch_from_rows;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Row {
        id: i64,
        name: String,
    }

    #[test]
    fn round_trip_through_tempfile() {
        let rows = vec![
            Row {
                id: 1,
                name: "Libra".into(),
            },
            Row {
                id: 2,
                name: "Taurus".into(),
            },
        ];
        let batch = batch_from_rows(&rows).unwrap().unwrap();
        let bytes = encode_parquet(&batch).unwrap();
        assert!(!bytes.is_empty());

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut tmp, &bytes).unwrap();
        let file = std::fs::File::open(tmp.path()).unwrap();
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap();
        let mut total = 0;
        for batch in reader {
            total += batch.unwrap().num_rows();
        }
        assert_eq!(total, 2);
    }
}
