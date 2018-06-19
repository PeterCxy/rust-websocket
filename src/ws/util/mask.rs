//! Utility functions for masking data frame payload data
use rand;
use std::io::Write;
use std::io::Result as IoResult;
use std::mem;

/// Struct to pipe data into another writer,
/// while masking the data being written
pub struct Masker<'w> {
	key: [u8; 4],
	pos: usize,
	end: &'w mut Write,
}

impl<'w> Masker<'w> {
	/// Create a new Masker with the key and the endpoint
	/// to be writer to.
	pub fn new(key: [u8; 4], endpoint: &'w mut Write) -> Self {
		Masker {
			key: key,
			pos: 0,
			end: endpoint,
		}
	}
}

impl<'w> Write for Masker<'w> {
	fn write(&mut self, data: &[u8]) -> IoResult<usize> {
    let buf = mask_data(self.key, &data);
		self.end.write(&buf)
	}

	fn flush(&mut self) -> IoResult<()> {
		self.end.flush()
	}
}

/// Generates a random masking key
pub fn gen_mask() -> [u8; 4] {
	// Faster than just calling random() many times
	unsafe { mem::transmute(rand::random::<u32>()) }
}

/// Masks data to send to a server and writes
pub fn mask_data(mask: [u8; 4], data: &[u8]) -> Vec<u8> {
	let mut out = Vec::with_capacity(data.len());
  unsafe { out.set_len(data.len()) };
	let zip_iter = data.iter().zip(mask.iter().cycle());
  let mut i = 0;
	for (&buf_item, &key_item) in zip_iter {
		out[i] = buf_item ^ key_item;
    i += 1;
	}
	out
}

#[cfg(all(feature = "nightly", test))]
mod tests {
	use super::*;
	use test;
	#[test]
	fn test_mask_data() {
		let key = [1u8, 2u8, 3u8, 4u8];
		let original = vec![10u8, 11u8, 12u8, 13u8, 14u8, 15u8, 16u8, 17u8];
		let expected = vec![11u8, 9u8, 15u8, 9u8, 15u8, 13u8, 19u8, 21u8];
		let obtained = mask_data(key, &original[..]);
		let reversed = mask_data(key, &obtained[..]);

		assert_eq!(original, reversed);
		assert_eq!(obtained, expected);
	}

	#[bench]
	fn bench_mask_data(b: &mut test::Bencher) {
		let buffer = b"The quick brown fox jumps over the lazy dog";
		let key = gen_mask();
		b.iter(|| {
			let mut output = mask_data(key, buffer);
			test::black_box(&mut output);
		});
	}

	#[bench]
	fn bench_gen_mask(b: &mut test::Bencher) {
		b.iter(|| {
			let mut key = gen_mask();
			test::black_box(&mut key);
		});
	}
}
