use crate::time::Instant;

/// Trait that describes the platform that this client runs on.
#[allow(async_fn_in_trait)]
pub trait Platform {
    /// Platform-specific error for all the platform methods.
    type Error;
    /// Connect the underlying transport. If already connected it should reconnect again.
    async fn connect_transport(&mut self) -> Result<(), Self::Error>;
    /// Write all the data in `buf` to the underlying transport.
    ///
    /// # Arguments
    ///
    ///   * `buf` - Buffer of data to write.
    ///
    /// # Returns
    ///
    ///   * `Ok(())` - All data was written
    ///   * `Err(e)` - An error ocurred. The client will treat this as the transport has closed.
    ///
    async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error>;
    /// Read some data into `buf`.
    ///
    /// **Note:** This should be cancel-safe for correct operation of the `NanoClient`.
    ///
    /// Buffer doesn't have to be filled, it is perfectly valid to return as soon as any bytes has been
    /// read, or on a timeout.
    ///
    /// # Arguments
    ///
    ///   * `buf` - Buffer to store the read data
    ///   * `timeout` - Timeout when the read operation should be aborted if no data has been received.
    ///
    /// # Returns
    ///
    ///   * `Ok(None)` - Timeout has expired.
    ///   * `Ok(Some(0))` - The underlying transport has closed (EOF). The client will treat this as transport has closed.
    ///   * `Ok(Some(n))` - `n` bytes were transferred to `buf`.
    ///   * `Err(_)` - An error has ocurred. The client will treat this as transport has closed.
    ///
    async fn read_some(
        &mut self,
        buf: &mut [u8],
        timeout: Option<core::time::Duration>,
    ) -> Result<Option<usize>, Self::Error>;
    /// Get the current time
    ///
    /// Gets the current time since some unspecified epoch.
    fn now(&mut self) -> Instant;
}