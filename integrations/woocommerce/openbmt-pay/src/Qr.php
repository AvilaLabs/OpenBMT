<?php
/**
 * A QR code encoder, ISO/IEC 18004 model 2, the port of the encoder of
 * `crates/openbmt-qr`: versions 1 to 40, levels L, M, Q and H, one segment
 * per code (numeric, alphanumeric or byte mode, the densest the text
 * allows), the smallest version that fits, and the mask with the lowest
 * penalty (ties to the lower number). The output is the module matrix and an
 * SVG drawn from nothing but that matrix.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Qr {

	const ECC_PER_BLOCK = array(
		'L' => array( 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30 ),
		'M' => array( 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28 ),
		'Q' => array( 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30 ),
		'H' => array( 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30 ),
	);

	const BLOCKS = array(
		'L' => array( 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25 ),
		'M' => array( 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49 ),
		'Q' => array( 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68 ),
		'H' => array( 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81 ),
	);

	const FORMAT_LEVEL = array(
		'L' => 1,
		'M' => 0,
		'Q' => 3,
		'H' => 2,
	);

	/**
	 * Encodes text.
	 *
	 * @param string $text The text, as bytes.
	 * @param string $ecc  The level: L, M, Q or H.
	 * @return array{size:int,version:int,mask:int,mode:string,modules:bool[]} The modules row by row, true for dark.
	 * @throws Refusal 'Level' for a level that is not one of the four, 'TooLong' for text no code holds.
	 */
	public static function encode( string $text, string $ecc ): array {
		if ( ! isset( self::FORMAT_LEVEL[ $ecc ] ) ) {
			throw new Refusal( 'Level' );
		}
		$mode  = self::detect( $text );
		$count = strlen( $text );
		$found = 0;
		for ( $v = 1; $v <= 40; $v++ ) {
			$capacity = self::data_codewords( $v, $ecc ) * 8;
			$needed   = 4 + self::count_bits( $mode, $v ) + self::data_bits( $mode, $count );
			if ( $needed <= $capacity && ( $count >> self::count_bits( $mode, $v ) ) === 0 ) {
				$found = $v;
				break;
			}
		}
		if ( 0 === $found ) {
			throw new Refusal( 'TooLong' );
		}
		$codewords = self::data_codewords( $found, $ecc );
		$bits      = self::segment_bits( $mode, $text, $found, $codewords * 8 );
		$data      = array();
		foreach ( array_chunk( $bits, 8 ) as $chunk ) {
			$byte = 0;
			foreach ( $chunk as $bit ) {
				$byte = ( $byte << 1 ) | ( $bit ? 1 : 0 );
			}
			$data[] = $byte;
		}
		$pad = array( 0xEC, 0x11 );
		for ( $i = 0; count( $data ) < $codewords; $i++ ) {
			$data[] = $pad[ $i % 2 ];
		}
		$all           = self::with_error_correction( $data, $found, $ecc );
		list( $dark, $mask ) = self::draw( $found, $ecc, $all );
		return array(
			'size'    => self::side( $found ),
			'version' => $found,
			'mask'    => $mask,
			'mode'    => $mode,
			'modules' => $dark,
		);
	}

	/**
	 * An SVG of the matrix: a light square and one path of the dark modules,
	 * with a quiet zone of four modules. It holds numbers and fixed text only.
	 *
	 * @param array{size:int,modules:bool[]} $code The code.
	 */
	public static function svg( array $code ): string {
		$quiet = 4;
		$size  = $code['size'];
		$total = $size + 2 * $quiet;
		$path  = '';
		for ( $y = 0; $y < $size; $y++ ) {
			$x = 0;
			while ( $x < $size ) {
				if ( ! $code['modules'][ $y * $size + $x ] ) {
					++$x;
					continue;
				}
				$run = 0;
				while ( $x + $run < $size && $code['modules'][ $y * $size + $x + $run ] ) {
					++$run;
				}
				$path .= 'M' . ( $x + $quiet ) . ' ' . ( $y + $quiet ) . 'h' . $run . 'v1h-' . $run . 'z';
				$x    += $run;
			}
		}
		return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ' . $total . ' ' . $total . '" shape-rendering="crispEdges">'
			. '<rect width="' . $total . '" height="' . $total . '" fill="#fff"/>'
			. '<path fill="#000" d="' . $path . '"/></svg>';
	}

	private static function detect( string $data ): string {
		if ( '' === $data || 1 === preg_match( '/^[0-9]*$/D', $data ) ) {
			return 'numeric';
		}
		if ( 1 === preg_match( '/^[0-9A-Z $%*+\-.\/:]*$/D', $data ) ) {
			return 'alphanumeric';
		}
		return 'byte';
	}

	private static function alnum_value( string $c ): int {
		$value = strpos( '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:', $c );
		return false === $value ? 0 : $value;
	}

	private static function indicator( string $mode ): int {
		return array(
			'numeric'      => 1,
			'alphanumeric' => 2,
			'byte'         => 4,
		)[ $mode ];
	}

	private static function count_bits( string $mode, int $version ): int {
		$sizes = array(
			'numeric'      => array( 10, 12, 14 ),
			'alphanumeric' => array( 9, 11, 13 ),
			'byte'         => array( 8, 16, 16 ),
		)[ $mode ];
		if ( $version <= 9 ) {
			return $sizes[0];
		}
		return $version <= 26 ? $sizes[1] : $sizes[2];
	}

	private static function numeric_tail_bits( int $n ): int {
		return array( 0, 4, 7, 10 )[ $n ];
	}

	private static function data_bits( string $mode, int $count ): int {
		if ( 'numeric' === $mode ) {
			return intdiv( $count, 3 ) * 10 + self::numeric_tail_bits( $count % 3 );
		}
		if ( 'alphanumeric' === $mode ) {
			return intdiv( $count, 2 ) * 11 + ( $count % 2 ) * 6;
		}
		return $count * 8;
	}

	private static function side( int $version ): int {
		return $version * 4 + 17;
	}

	private static function raw_modules( int $v ): int {
		$n = ( 16 * $v + 128 ) * $v + 64;
		if ( $v >= 2 ) {
			$a  = intdiv( $v, 7 ) + 2;
			$n -= ( 25 * $a - 10 ) * $a - 55;
			if ( $v >= 7 ) {
				$n -= 36;
			}
		}
		return $n;
	}

	private static function total_codewords( int $v ): int {
		return intdiv( self::raw_modules( $v ), 8 );
	}

	private static function data_codewords( int $v, string $ecc ): int {
		return self::total_codewords( $v ) - self::ECC_PER_BLOCK[ $ecc ][ $v - 1 ] * self::BLOCKS[ $ecc ][ $v - 1 ];
	}

	/**
	 * @return int[]
	 */
	private static function alignment_positions( int $v ): array {
		if ( $v <= 1 ) {
			return array();
		}
		$n    = intdiv( $v, 7 ) + 2;
		$step = 32 === $v ? 26 : intdiv( $v * 4 + $n * 2 + 1, $n * 2 - 2 ) * 2;
		$out  = array( 6 );
		$pos  = self::side( $v ) - 7;
		for ( $i = 1; $i < $n; $i++ ) {
			array_splice( $out, 1, 0, array( $pos ) );
			$pos = max( 0, $pos - $step );
		}
		return $out;
	}

	/**
	 * @return bool[]
	 */
	private static function segment_bits( string $mode, string $data, int $version, int $capacity ): array {
		$out   = array();
		$push  = static function ( int $value, int $n ) use ( &$out ): void {
			for ( $i = $n - 1; $i >= 0; $i-- ) {
				$out[] = ( ( $value >> $i ) & 1 ) !== 0;
			}
		};
		$count = strlen( $data );
		$push( self::indicator( $mode ), 4 );
		$push( $count, self::count_bits( $mode, $version ) );
		if ( 'numeric' === $mode ) {
			foreach ( str_split( $data, 3 ) as $group ) {
				if ( '' !== $group ) {
					$push( (int) $group, self::numeric_tail_bits( strlen( $group ) ) );
				}
			}
		} elseif ( 'alphanumeric' === $mode ) {
			foreach ( str_split( $data, 2 ) as $pair ) {
				if ( '' === $pair ) {
					continue;
				}
				if ( 2 === strlen( $pair ) ) {
					$push( self::alnum_value( $pair[0] ) * 45 + self::alnum_value( $pair[1] ), 11 );
				} else {
					$push( self::alnum_value( $pair[0] ), 6 );
				}
			}
		} else {
			for ( $i = 0; $i < $count; $i++ ) {
				$push( ord( $data[ $i ] ), 8 );
			}
		}
		$terminator = min( 4, max( 0, $capacity - count( $out ) ) );
		for ( $i = 0; $i < $terminator; $i++ ) {
			$out[] = false;
		}
		while ( 0 !== count( $out ) % 8 ) {
			$out[] = false;
		}
		return $out;
	}

	private static function gf_mul( int $x, int $y ): int {
		$z = 0;
		for ( $i = 7; $i >= 0; $i-- ) {
			$z  = ( $z << 1 ) ^ ( ( $z >> 7 ) * 0x11D );
			$z ^= ( ( $y >> $i ) & 1 ) * $x;
		}
		return $z & 0xFF;
	}

	/**
	 * @return int[]
	 */
	private static function divisor( int $degree ): array {
		$result              = array_fill( 0, $degree, 0 );
		$result[ $degree - 1 ] = 1;
		$root                = 1;
		for ( $i = 0; $i < $degree; $i++ ) {
			for ( $j = 0; $j < $degree; $j++ ) {
				$next          = $result[ $j + 1 ] ?? 0;
				$result[ $j ] = self::gf_mul( $result[ $j ], $root ) ^ $next;
			}
			$root = self::gf_mul( $root, 2 );
		}
		return $result;
	}

	/**
	 * @param int[] $data    One block of data codewords.
	 * @param int[] $divisor The generator.
	 * @return int[]
	 */
	private static function remainder( array $data, array $divisor ): array {
		$result = array_fill( 0, count( $divisor ), 0 );
		foreach ( $data as $b ) {
			$factor = $b ^ $result[0];
			array_shift( $result );
			$result[] = 0;
			foreach ( $divisor as $i => $y ) {
				$result[ $i ] ^= self::gf_mul( $y, $factor );
			}
		}
		return $result;
	}

	/**
	 * @param int[] $data The data codewords.
	 * @return int[]
	 */
	private static function with_error_correction( array $data, int $version, string $ecc ): array {
		$n_blocks  = self::BLOCKS[ $ecc ][ $version - 1 ];
		$ecc_len   = self::ECC_PER_BLOCK[ $ecc ][ $version - 1 ];
		$raw       = self::total_codewords( $version );
		$n_short   = $n_blocks - $raw % $n_blocks;
		$short_len = intdiv( $raw, $n_blocks );
		$div       = self::divisor( $ecc_len );
		$blocks    = array();
		$ecc_parts = array();
		$offset    = 0;
		for ( $i = 0; $i < $n_blocks; $i++ ) {
			$len         = min( $short_len - $ecc_len + ( $i >= $n_short ? 1 : 0 ), count( $data ) - $offset );
			$block       = array_slice( $data, $offset, $len );
			$offset     += $len;
			$blocks[]    = $block;
			$ecc_parts[] = self::remainder( $block, $div );
		}
		$out     = array();
		$longest = 0;
		foreach ( $blocks as $b ) {
			$longest = max( $longest, count( $b ) );
		}
		for ( $i = 0; $i < $longest; $i++ ) {
			foreach ( $blocks as $b ) {
				if ( isset( $b[ $i ] ) ) {
					$out[] = $b[ $i ];
				}
			}
		}
		for ( $i = 0; $i < $ecc_len; $i++ ) {
			foreach ( $ecc_parts as $b ) {
				$out[] = $b[ $i ];
			}
		}
		return $out;
	}

	private static function format_bits( string $ecc, int $mask ): int {
		$data = ( self::FORMAT_LEVEL[ $ecc ] << 3 ) | ( $mask & 7 );
		$rem  = $data;
		for ( $i = 0; $i < 10; $i++ ) {
			$rem = ( $rem << 1 ) ^ ( ( $rem >> 9 ) * 0x537 );
		}
		return ( ( $data << 10 ) | $rem ) ^ 0x5412;
	}

	private static function version_bits( int $version ): int {
		$rem = $version;
		for ( $i = 0; $i < 12; $i++ ) {
			$rem = ( $rem << 1 ) ^ ( ( $rem >> 11 ) * 0x1F25 );
		}
		return ( $version << 12 ) | $rem;
	}

	private static function mask_bit( int $mask, int $x, int $y ): bool {
		switch ( $mask ) {
			case 0:
				return 0 === ( $x + $y ) % 2;
			case 1:
				return 0 === $y % 2;
			case 2:
				return 0 === $x % 3;
			case 3:
				return 0 === ( $x + $y ) % 3;
			case 4:
				return 0 === ( intdiv( $x, 3 ) + intdiv( $y, 2 ) ) % 2;
			case 5:
				return 0 === ( $x * $y ) % 2 + ( $x * $y ) % 3;
			case 6:
				return 0 === ( ( $x * $y ) % 2 + ( $x * $y ) % 3 ) % 2;
			default:
				return 0 === ( ( $x + $y ) % 2 + ( $x * $y ) % 3 ) % 2;
		}
	}

	/**
	 * Sets a function module; coordinates outside the square are ignored.
	 *
	 * @param bool[] $dark Modules.
	 * @param bool[] $func Which are function modules.
	 */
	private static function set_func( array &$dark, array &$func, int $size, int $x, int $y, bool $on ): void {
		if ( $x < 0 || $y < 0 || $x >= $size || $y >= $size ) {
			return;
		}
		$dark[ $y * $size + $x ] = $on;
		$func[ $y * $size + $x ] = true;
	}

	/**
	 * @param bool[] $dark Modules.
	 * @param bool[] $func Which are function modules.
	 */
	private static function draw_format( array &$dark, array &$func, int $size, string $ecc, int $mask ): void {
		$bits = self::format_bits( $ecc, $mask );
		$bit  = static function ( int $i ) use ( $bits ): bool {
			return ( ( $bits >> $i ) & 1 ) !== 0;
		};
		for ( $i = 0; $i < 6; $i++ ) {
			self::set_func( $dark, $func, $size, 8, $i, $bit( $i ) );
		}
		self::set_func( $dark, $func, $size, 8, 7, $bit( 6 ) );
		self::set_func( $dark, $func, $size, 8, 8, $bit( 7 ) );
		self::set_func( $dark, $func, $size, 7, 8, $bit( 8 ) );
		for ( $i = 9; $i < 15; $i++ ) {
			self::set_func( $dark, $func, $size, 14 - $i, 8, $bit( $i ) );
		}
		for ( $i = 0; $i < 8; $i++ ) {
			self::set_func( $dark, $func, $size, $size - 1 - $i, 8, $bit( $i ) );
		}
		for ( $i = 8; $i < 15; $i++ ) {
			self::set_func( $dark, $func, $size, 8, $size - 15 + $i, $bit( $i ) );
		}
		self::set_func( $dark, $func, $size, 8, $size - 8, true );
	}

	/**
	 * @param bool[] $dark Modules.
	 * @param bool[] $func Which are function modules.
	 */
	private static function draw_function_patterns( array &$dark, array &$func, int $size, int $version ): void {
		for ( $i = 0; $i < $size; $i++ ) {
			self::set_func( $dark, $func, $size, 6, $i, 0 === $i % 2 );
			self::set_func( $dark, $func, $size, $i, 6, 0 === $i % 2 );
		}
		foreach ( array( array( 3, 3 ), array( $size - 4, 3 ), array( 3, $size - 4 ) ) as $c ) {
			for ( $dy = -4; $dy <= 4; $dy++ ) {
				for ( $dx = -4; $dx <= 4; $dx++ ) {
					$dist = max( abs( $dx ), abs( $dy ) );
					self::set_func( $dark, $func, $size, $c[0] + $dx, $c[1] + $dy, 2 !== $dist && 4 !== $dist );
				}
			}
		}
		$pos  = self::alignment_positions( $version );
		$last = count( $pos ) - 1;
		foreach ( $pos as $i => $a ) {
			foreach ( $pos as $j => $b ) {
				$corner = ( 0 === $i && 0 === $j ) || ( 0 === $i && $j === $last ) || ( $i === $last && 0 === $j );
				if ( $corner ) {
					continue;
				}
				for ( $dy = -2; $dy <= 2; $dy++ ) {
					for ( $dx = -2; $dx <= 2; $dx++ ) {
						self::set_func( $dark, $func, $size, $a + $dx, $b + $dy, 1 !== max( abs( $dx ), abs( $dy ) ) );
					}
				}
			}
		}
		self::draw_format( $dark, $func, $size, 'L', 0 );
		if ( $version >= 7 ) {
			$bits = self::version_bits( $version );
			for ( $i = 0; $i < 18; $i++ ) {
				$on = ( ( $bits >> $i ) & 1 ) !== 0;
				$a  = $size - 11 + $i % 3;
				$b  = intdiv( $i, 3 );
				self::set_func( $dark, $func, $size, $a, $b, $on );
				self::set_func( $dark, $func, $size, $b, $a, $on );
			}
		}
	}

	/**
	 * @param int[] $codewords All the codewords.
	 * @return array{0:bool[],1:int} The modules and the mask used.
	 */
	private static function draw( int $version, string $ecc, array $codewords ): array {
		$size = self::side( $version );
		$dark = array_fill( 0, $size * $size, false );
		$func = array_fill( 0, $size * $size, false );
		self::draw_function_patterns( $dark, $func, $size, $version );
		// The codeword bits in the zigzag of the standard, skipping function modules.
		$total = count( $codewords ) * 8;
		$i     = 0;
		$right = $size - 1;
		while ( $right >= 1 ) {
			if ( 6 === $right ) {
				$right = 5;
			}
			for ( $vert = 0; $vert < $size; $vert++ ) {
				for ( $j = 0; $j < 2; $j++ ) {
					$x      = $right - $j;
					$upward = ( ( $right + 1 ) & 2 ) === 0;
					$y      = $upward ? $size - 1 - $vert : $vert;
					if ( ! $func[ $y * $size + $x ] && $i < $total ) {
						$dark[ $y * $size + $x ] = ( ( $codewords[ $i >> 3 ] >> ( 7 - ( $i & 7 ) ) ) & 1 ) !== 0;
						++$i;
					}
				}
			}
			$right -= 2;
		}
		$best      = null;
		$best_mask = 0;
		$best_pen  = 0;
		for ( $m = 0; $m < 8; $m++ ) {
			$g  = $dark;
			$gf = $func;
			for ( $y = 0; $y < $size; $y++ ) {
				for ( $x = 0; $x < $size; $x++ ) {
					$k = $y * $size + $x;
					if ( ! $gf[ $k ] && self::mask_bit( $m, $x, $y ) ) {
						$g[ $k ] = ! $g[ $k ];
					}
				}
			}
			self::draw_format( $g, $gf, $size, $ecc, $m );
			$p = self::penalty( $size, $g );
			if ( null === $best || $p < $best_pen ) {
				$best      = $g;
				$best_mask = $m;
				$best_pen  = $p;
			}
		}
		return array( $best, $best_mask );
	}

	/**
	 * @param bool[] $line A row or column.
	 */
	private static function run_penalty( array $line ): int {
		$total = 0;
		$run   = 0;
		$prev  = null;
		foreach ( $line as $m ) {
			if ( $prev === $m ) {
				++$run;
			} else {
				$run  = 1;
				$prev = $m;
			}
			if ( 5 === $run ) {
				$total += 3;
			} elseif ( $run > 5 ) {
				++$total;
			}
		}
		return $total;
	}

	/**
	 * @param bool[] $line A row or column.
	 */
	private static function finder_like_penalty( array $line ): int {
		$core  = array( true, false, true, true, true, false, true );
		$len   = count( $line );
		$total = 0;
		for ( $start = 0; $start < $len - 6; $start++ ) {
			if ( array_slice( $line, $start, 7 ) !== $core ) {
				continue;
			}
			$before = false;
			if ( $start >= 4 ) {
				$before = ! ( $line[ $start - 4 ] || $line[ $start - 3 ] || $line[ $start - 2 ] || $line[ $start - 1 ] );
			}
			$after = false;
			if ( $start + 11 <= $len ) {
				$after = ! ( $line[ $start + 7 ] || $line[ $start + 8 ] || $line[ $start + 9 ] || $line[ $start + 10 ] );
			}
			if ( $before || $after ) {
				$total += 40;
			}
		}
		return $total;
	}

	/**
	 * @param bool[] $modules The modules, row by row.
	 */
	private static function penalty( int $size, array $modules ): int {
		$total = 0;
		$rows  = array_chunk( $modules, $size );
		foreach ( $rows as $row ) {
			$total += self::run_penalty( $row ) + self::finder_like_penalty( $row );
		}
		for ( $x = 0; $x < $size; $x++ ) {
			$col = array();
			for ( $y = 0; $y < $size; $y++ ) {
				$col[] = $modules[ $y * $size + $x ];
			}
			$total += self::run_penalty( $col ) + self::finder_like_penalty( $col );
		}
		for ( $y = 0; $y < $size - 1; $y++ ) {
			for ( $x = 0; $x < $size - 1; $x++ ) {
				$p = $rows[ $y ][ $x ];
				if ( $p === $rows[ $y ][ $x + 1 ] && $p === $rows[ $y + 1 ][ $x ] && $p === $rows[ $y + 1 ][ $x + 1 ] ) {
					$total += 3;
				}
			}
		}
		$dark = 0;
		foreach ( $modules as $m ) {
			if ( $m ) {
				++$dark;
			}
		}
		$low    = intdiv( $dark * 20, count( $modules ) );
		$steps  = min( abs( $low - 10 ), abs( $low + 1 - 10 ) );
		$total += $steps * 10;
		return $total;
	}
}
