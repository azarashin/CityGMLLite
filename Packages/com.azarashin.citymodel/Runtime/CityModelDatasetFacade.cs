using System;
using System.Threading;
using System.Threading.Tasks;
using CityModel.Loading;

namespace CityModel
{
    /// <summary>Application-facing lifecycle facade that safely owns one opened dataset.</summary>
    public sealed class CityModelDatasetFacade : IAsyncDisposable, IDisposable
    {
        private CityModelDataset _dataset;
        private int _closed;
        private CityModelDatasetFacade(CityModelDataset dataset) { _dataset = dataset; }
        public DatasetManifest Manifest => _dataset?.Manifest ?? throw new ObjectDisposedException(nameof(CityModelDatasetFacade));
        public static async Task<CityModelDatasetFacade> OpenAsync(string rootDirectory, CancellationToken cancellationToken)
        {
            CityModelDataset dataset = null;
            try { dataset = await CityModelDataset.OpenAsync(rootDirectory, cancellationToken).ConfigureAwait(false); return new CityModelDatasetFacade(dataset); }
            catch { dataset?.Dispose(); throw; }
        }
        public Task CloseAsync() { Dispose(); return Task.CompletedTask; }
        public ValueTask DisposeAsync() { Dispose(); return new ValueTask(Task.CompletedTask); }
        public void Dispose() { if (Interlocked.Exchange(ref _closed, 1) == 0) { _dataset?.Dispose(); _dataset = null; } }
    }
}
