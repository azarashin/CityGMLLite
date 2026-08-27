using System;
using System.Threading;
using System.Threading.Tasks;
using CityModel.Loading;
using UnityEngine;

namespace CityModel.Samples
{
    /// <summary>Minimal sample entry point. Set Dataset Root to a converter output directory.</summary>
    public sealed class QuickStartLoader : MonoBehaviour
    {
        [Tooltip("Absolute path to a directory containing dataset.manifest.json.")]
        [SerializeField] private string datasetRoot;

        private CityModelDataset _dataset;
        private CancellationTokenSource _cancellation;

        private async void Start()
        {
            if (string.IsNullOrWhiteSpace(datasetRoot))
            {
                Debug.Log("CityModel Quick Start: set Dataset Root in the Inspector, then press Play.");
                return;
            }
            _cancellation = new CancellationTokenSource();
            try
            {
                _dataset = await CityModelDataset.OpenAsync(datasetRoot, _cancellation.Token);
                Debug.Log($"CityModel dataset opened: {_dataset.Manifest.datasetId} ({_dataset.Manifest.generationId})");
            }
            catch (Exception exception)
            {
                Debug.LogException(exception, this);
            }
        }

        private void OnDestroy()
        {
            _cancellation?.Cancel();
            _cancellation?.Dispose();
            _dataset?.Dispose();
        }
    }
}
